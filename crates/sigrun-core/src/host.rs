//! The headless Sigrun host: owns the vault, the agent, and the TOML store,
//! and serves every CLI request over the local IPC socket.

use std::path::{Path, PathBuf};

use crate::agent::{vault_keys, AgentState};
use crate::error::{Error, Result};
use crate::ipc::protocol::{error_code, Request, Response, StatusInfo};
use crate::keys::{self, GeneratedKey, ImportedKey};
use crate::model::{Connection, KeyAlgorithm, KeyMetadata, KeySource, KeyView};
use crate::paths::{ensure_private_dir, Paths};
use crate::ssh_config::parse_ssh_config;
use crate::store::Store;
use crate::terminal::{
    env_prefix, launch_first, probe_reachable, render_command, ssh_invocation,
    terminal_launch_specs, Platform,
};
use crate::vault::{Secret, UnlockedVault, Vault, VaultStatus};
use zeroize::Zeroizing;

/// Long-lived host state. The lock-file handle stays open for the host's
/// lifetime so no second host can start against the same data directory.
pub struct HostState {
    pub paths: Paths,
    pub store: Store,
    pub vault: tokio::sync::Mutex<Option<UnlockedVault>>,
    pub agent: AgentState,
    pub agent_socket: String,
    /// The IPC address this host actually serves on. `Paths::socket_path()` is
    /// only the default: `sigrun host --socket …` overrides it, and the askpass
    /// prefix must advertise the address the helper can really connect to.
    pub ipc_addr: String,
    pub shutdown: tokio_util::sync::CancellationToken,
    /// Bumped on any vault-touching activity; the idle auto-lock watches it.
    pub last_vault_activity: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// Set when the vault is locked by the idle auto-lock (the desktop host
    /// reads it to re-gate the UI). Never set on a manual lock.
    pub auto_locked: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub lock_file: std::fs::File,
}

impl HostState {
    pub async fn new(paths: Paths) -> Result<HostState> {
        ensure_private_dir(&paths.config_dir)?;
        ensure_private_dir(&paths.data_dir)?;

        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(paths.host_lock_file())?;
        fs2::FileExt::try_lock_exclusive(&lock_file).map_err(|_| Error::AlreadyRunning)?;

        Ok(HostState {
            agent_socket: paths.agent_socket().display().to_string(),
            ipc_addr: paths.socket_path().display().to_string(),
            store: Store::new(paths.clone()),
            vault: tokio::sync::Mutex::new(None),
            agent: AgentState::new(),
            shutdown: tokio_util::sync::CancellationToken::new(),
            last_vault_activity: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            auto_locked: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            paths,
            lock_file,
        })
    }

    /// Lock the vault after `auto_lock_minutes` of no vault-touching activity.
    /// Runs until `shutdown` is cancelled; a `0` disables it. On fire it locks
    /// and sets `auto_locked` so the caller can re-gate the UI.
    pub async fn watch_auto_lock(self: &std::sync::Arc<Self>) {
        use std::sync::atomic::Ordering;
        loop {
            // Poll once a minute, or return on shutdown.
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {}
                _ = self.shutdown.cancelled() => return,
            }
            let minutes = self
                .store
                .load_settings()
                .map(|s| s.auto_lock_minutes)
                .unwrap_or(0);
            if minutes == 0 {
                continue;
            }
            if self.vault.lock().await.is_none() {
                continue; // nothing to lock
            }
            let last = self.last_vault_activity.load(Ordering::Relaxed);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let idle_for = now.saturating_sub(last);
            if last != 0 && idle_for >= u64::from(minutes) * 60 {
                *self.vault.lock().await = None;
                self.agent.clear();
                self.auto_locked.store(true, Ordering::Relaxed);
            }
        }
    }
}

/// Dispatch a request, mapping any internal error to a safe `Error` response.
pub async fn handle(state: &HostState, req: Request) -> Response {
    match handle_inner(state, req).await {
        Ok(resp) => resp,
        Err(e) => Response::Error {
            code: error_code(&e).to_string(),
            message: e.to_string(),
        },
    }
}

async fn handle_inner(state: &HostState, req: Request) -> Result<Response> {
    // Reset the idle auto-lock clock on any request that touches the vault.
    if matches!(
        req,
        Request::VaultInit { .. }
            | Request::VaultUnlock { .. }
            | Request::VaultLock
            | Request::KeysGenerate { .. }
            | Request::KeysImport { .. }
            | Request::KeysRemove { .. }
            | Request::ConnectionsConnect { .. }
            | Request::ConnectionsProvision { .. }
            | Request::ConnectionsSetPassword { .. }
    ) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        state
            .last_vault_activity
            .store(now, std::sync::atomic::Ordering::Relaxed);
    }
    match req {
        Request::Status => {
            let vault = vault_status(state).await;
            Ok(Response::Status(StatusInfo {
                version: crate::VERSION.to_string(),
                vault,
                agent_socket: Some(state.agent_socket.clone()),
                host_addr: state.ipc_addr.clone(),
            }))
        }
        Request::VaultInit { passphrase } => {
            if state.paths.vault_file().exists() {
                return Err(Error::AlreadyExists("vault".into()));
            }
            let vault_path = state.paths.vault_file().clone();
            tokio::task::spawn_blocking(move || {
                let vault = Vault::new(vault_path);
                vault.create(&passphrase)
            })
            .await
            .map_err(|e| Error::Ipc(e.to_string()))??;
            Ok(Response::Ok)
        }
        Request::VaultUnlock { passphrase } => {
            let vault_path = state.paths.vault_file().clone();
            let unlocked =
                tokio::task::spawn_blocking(move || Vault::new(vault_path).unlock(&passphrase))
                    .await
                    .map_err(|e| Error::Ipc(e.to_string()))??;
            *state.vault.lock().await = Some(unlocked);
            let warnings = reload_agent(state).await?;
            Ok(Response::Unlocked { warnings })
        }
        Request::VaultLock => {
            *state.vault.lock().await = None;
            state.agent.clear();
            Ok(Response::Ok)
        }
        Request::ConnectionsList => {
            let mut connections = state.store.load_data()?.connections;
            fill_password_flags(state, &mut connections).await;
            Ok(Response::Connections(connections))
        }
        Request::ConnectionsAdd { mut connection } => {
            if connection.id.trim().is_empty() {
                connection.id = uuid::Uuid::new_v4().to_string();
            }
            // `has_password` is host-derived. Clearing it keeps anything a
            // client sends out of `data.toml`; readers fill it from the vault.
            connection.has_password = false;
            state.store.upsert_connection(connection)?;
            Ok(Response::Ok)
        }
        Request::ConnectionsUpdate { mut connection } => {
            connection.has_password = false;
            state.store.upsert_connection(connection)?;
            Ok(Response::Ok)
        }
        Request::ConnectionsRemove { id } => {
            // Clear the stored password while we still can. A locked vault
            // cannot reach it, so the connection is removed anyway rather than
            // blocking the delete on an unlock.
            {
                let mut guard = state.vault.lock().await;
                if let Some(vault) = (*guard).as_mut() {
                    vault.remove(&id)?;
                }
            }
            state.store.remove_connection(&id)?;
            Ok(Response::Ok)
        }
        Request::ConnectionsSetPassword { id, password } => {
            connections_set_password(state, id, password).await
        }
        Request::PasswordGet { id } => password_get(state, id).await,
        Request::KeysList => keys_list(state).await,
        Request::KeysGenerate {
            name,
            algorithm,
            passphrase,
        } => keys_generate(state, name, algorithm, passphrase).await,
        Request::KeysImport {
            path,
            name,
            passphrase,
        } => keys_import(state, path, name, passphrase).await,
        Request::KeysRemove { id } => keys_remove(state, id).await,
        Request::TerminalsList => Ok(Response::Terminals(crate::terminal::detect_terminals(
            crate::terminal::Platform::current(),
        ))),
        Request::ConnectionsConnect {
            id,
            print_only,
            force,
        } => connections_connect(state, id, print_only, force).await,
        Request::ConnectionsProvision {
            id,
            password,
            key_ids,
            disable_password_auth,
        } => connections_provision(state, id, password, key_ids, disable_password_auth).await,
        Request::Shutdown => {
            state.shutdown.cancel();
            Ok(Response::Ok)
        }
    }
}

async fn vault_status(state: &HostState) -> VaultStatus {
    if state.vault.lock().await.is_some() {
        VaultStatus::Unlocked
    } else if state.paths.vault_file().exists() {
        VaultStatus::Locked
    } else {
        VaultStatus::Uninitialized
    }
}

/// Rebuild the agent's key set from the unlocked vault. Returns per-key load
/// warnings (never secret material). With a locked vault this just clears the
/// agent.
async fn reload_agent(state: &HostState) -> Result<Vec<String>> {
    let guard = state.vault.lock().await;
    let Some(vault) = &*guard else {
        state.agent.clear();
        return Ok(vec![]);
    };
    let metadata = state.store.load_data()?.keys;
    let (keys, warnings) = vault_keys(vault, &metadata);
    state.agent.replace(keys);
    Ok(warnings)
}

async fn keys_list(state: &HostState) -> Result<Response> {
    let mut views = sync_disk_keys(&state.store, &state.paths.ssh_dir)?;
    let guard = state.vault.lock().await;
    let unlocked = guard.is_some();
    let data = state.store.load_data()?;
    for key in data.keys {
        if key.source == KeySource::Vault {
            let available = unlocked && (*guard).as_ref().is_some_and(|v| v.get(&key.id).is_some());
            views.push(KeyView { key, available });
        }
    }
    views.sort_by(|a, b| (&a.key.name, &a.key.fingerprint).cmp(&(&b.key.name, &b.key.fingerprint)));
    Ok(Response::Keys(views))
}

async fn keys_generate(
    state: &HostState,
    name: String,
    algorithm: KeyAlgorithm,
    passphrase: Option<String>,
) -> Result<Response> {
    if state.vault.lock().await.is_none() {
        return Err(Error::VaultLocked);
    }

    let id = uuid::Uuid::new_v4().to_string();
    let has_passphrase = passphrase.is_some();
    let GeneratedKey {
        private_pem,
        public_openssh,
        fingerprint,
        ..
    } = keys::generate(algorithm, passphrase.as_deref())?;

    let meta = {
        let mut guard = state.vault.lock().await;
        let vault = (*guard).as_mut().ok_or(Error::VaultLocked)?;
        vault.put(
            &id,
            Secret::PrivateKey {
                pem: private_pem,
                passphrase,
            },
        )?;
        let mut meta = KeyMetadata::now(
            id.clone(),
            algorithm,
            public_openssh,
            fingerprint,
            KeySource::Vault,
        );
        meta.name = name;
        meta.has_passphrase = has_passphrase;
        meta
    };

    state.store.upsert_key(meta.clone())?;
    reload_agent(state).await?;
    Ok(Response::Key(KeyView {
        key: meta,
        available: true,
    }))
}

async fn keys_import(
    state: &HostState,
    path: PathBuf,
    name: String,
    passphrase: Option<String>,
) -> Result<Response> {
    if state.vault.lock().await.is_none() {
        return Err(Error::VaultLocked);
    }

    let id = uuid::Uuid::new_v4().to_string();
    let ImportedKey {
        private_pem,
        public_key,
        fingerprint,
        algorithm,
    } = keys::import_private_key(&path, passphrase.as_deref())?;

    {
        let mut guard = state.vault.lock().await;
        let vault = (*guard).as_mut().ok_or(Error::VaultLocked)?;
        // The vault copy is already decrypted, so it carries no passphrase.
        vault.put(
            &id,
            Secret::PrivateKey {
                pem: private_pem,
                passphrase: None,
            },
        )?;
    }

    let mut meta = KeyMetadata::now(
        id.clone(),
        algorithm,
        public_key,
        fingerprint,
        KeySource::Vault,
    );
    meta.name = name;
    meta.has_passphrase = false;
    state.store.upsert_key(meta.clone())?;
    reload_agent(state).await?;
    Ok(Response::Key(KeyView {
        key: meta,
        available: true,
    }))
}

async fn keys_remove(state: &HostState, id: String) -> Result<Response> {
    let data = state.store.load_data()?;
    let meta = data
        .keys
        .iter()
        .find(|k| k.id == id)
        .cloned()
        .ok_or_else(|| Error::NotFound(id.clone()))?;

    if meta.source == KeySource::Vault {
        // Never half-delete: require the vault before touching either store.
        let mut guard = state.vault.lock().await;
        let vault = (*guard).as_mut().ok_or(Error::VaultLocked)?;
        vault.remove(&id)?;
    }

    state.store.remove_key(&id)?;
    reload_agent(state).await?;
    Ok(Response::Ok)
}

/// Fill `has_password` for each connection from the unlocked vault. A locked
/// vault cannot answer, so the flag stays `false` rather than guess.
async fn fill_password_flags(state: &HostState, connections: &mut [Connection]) {
    let guard = state.vault.lock().await;
    for conn in connections {
        with_password_flag((*guard).as_ref(), conn);
    }
}

/// The host-owned half of `Connection.has_password`.
fn with_password_flag(vault: Option<&UnlockedVault>, conn: &mut Connection) {
    conn.has_password =
        vault.is_some_and(|v| matches!(v.get(&conn.id), Some(Secret::Password { .. })));
}

/// Store or clear a connection's password under its id. `None` clears; an absent
/// entry is a silent no-op. The value is never logged or echoed in an error.
async fn connections_set_password(
    state: &HostState,
    id: String,
    password: Option<String>,
) -> Result<Response> {
    let mut guard = state.vault.lock().await;
    let vault = (*guard).as_mut().ok_or(Error::VaultLocked)?;
    if !state
        .store
        .load_data()?
        .connections
        .iter()
        .any(|c| c.id == id)
    {
        return Err(Error::NotFound(id));
    }
    match password {
        Some(password) => {
            if password.is_empty() {
                return Err(Error::InvalidInput("password must not be empty".into()));
            }
            vault.put(&id, Secret::Password { password })?;
        }
        None => vault.remove(&id)?,
    }
    Ok(Response::Ok)
}

/// Hand a stored password to the caller. The only request that ever returns a
/// secret; it travels to the 0600 socket exactly as `VaultUnlock`'s passphrase.
async fn password_get(state: &HostState, id: String) -> Result<Response> {
    let guard = state.vault.lock().await;
    let vault = (*guard).as_ref().ok_or(Error::VaultLocked)?;
    match vault.get(&id) {
        Some(Secret::Password { password }) => Ok(Response::Password(password.clone())),
        _ => Err(Error::NotFound(id)),
    }
}

async fn connections_connect(
    state: &HostState,
    id: String,
    print_only: bool,
    force: bool,
) -> Result<Response> {
    let data = state.store.load_data()?;
    let mut conn = data
        .connections
        .iter()
        .find(|c| c.id == id)
        .or_else(|| data.connections.iter().find(|c| c.label == id))
        .cloned()
        .ok_or_else(|| Error::NotFound(id.clone()))?;
    let settings = state.store.load_settings()?;

    let meta = match &conn.key_id {
        None => None,
        Some(key_id) => {
            let meta = data
                .keys
                .iter()
                .find(|k| &k.id == key_id)
                .cloned()
                .ok_or_else(|| Error::NotFound(key_id.clone()))?;
            if meta.source == KeySource::Vault {
                if state.vault.lock().await.is_none() {
                    return Err(Error::VaultLocked);
                }
                if !state.agent.contains_key_id(key_id) {
                    reload_agent(state).await?;
                    if !state.agent.contains_key_id(key_id) {
                        return Err(Error::Key("key could not be loaded into the agent".into()));
                    }
                }
            }
            Some(meta)
        }
    };

    // Key and vault are validated first (unchanged order); only then does the
    // host get contacted, so a bad key never costs a probe round trip.
    if !print_only && !force {
        probe_reachable(&conn.host, conn.port, std::time::Duration::from_secs(3)).await?;
    }

    // The store never persists `has_password`, so fill it from the vault now:
    // it drives both the ssh argv and the askpass prefix. A locked vault cannot
    // answer, so the connection stays password-less and ssh prompts as before.
    let stored = {
        let guard = state.vault.lock().await;
        (*guard)
            .as_ref()
            .is_some_and(|v| matches!(v.get(&conn.id), Some(Secret::Password { .. })))
    };
    conn.has_password = stored;

    let argv = ssh_invocation(&conn, meta.as_ref())?;
    if print_only {
        return Ok(Response::Command(render_command(&argv)));
    }

    let prefix = if stored {
        // The host may serve on `--socket`, so advertise the bound address.
        env_prefix(Path::new(&state.ipc_addr), &conn.id)
    } else {
        String::new()
    };

    let specs = terminal_launch_specs(
        Platform::current(),
        settings.terminal.as_deref(),
        &argv,
        &prefix,
    )?;
    launch_first(&specs, Some(&state.paths.agent_socket()))?;
    Ok(Response::Ok)
}

async fn connections_provision(
    state: &HostState,
    id: String,
    password: String,
    key_ids: Vec<String>,
    disable_password_auth: bool,
) -> Result<Response> {
    use crate::provision::{install_keys, ProvisionKey, ProvisionOptions};

    let data = state.store.load_data()?;
    let conn = match data.connections.iter().find(|c| c.id == id) {
        Some(c) => c.clone(),
        None => {
            let matches: Vec<_> = data.connections.iter().filter(|c| c.label == id).collect();
            match matches.as_slice() {
                [] => return Err(Error::NotFound(id)),
                [one] => (*one).clone(),
                many => {
                    let ids: Vec<_> = many.iter().map(|c| c.id.as_str()).collect();
                    return Err(Error::InvalidInput(format!(
                        "label '{id}' is ambiguous; matching ids: {}",
                        ids.join(", ")
                    )));
                }
            }
        }
    };

    let key_ids = if key_ids.is_empty() {
        conn.key_id.clone().into_iter().collect()
    } else {
        key_ids
    };
    if key_ids.is_empty() {
        return Err(Error::InvalidInput("no key selected".into()));
    }

    let mut keys = Vec::new();
    {
        let guard = state.vault.lock().await;
        let vault = (*guard).as_ref().ok_or(Error::VaultLocked)?;
        for key_id in &key_ids {
            let meta = data
                .keys
                .iter()
                .find(|k| &k.id == key_id)
                .ok_or_else(|| Error::NotFound(key_id.clone()))?;
            if meta.source != KeySource::Vault {
                return Err(Error::InvalidInput(
                    "import the key into the vault to provision it".into(),
                ));
            }
            let (pem, passphrase) = match vault.get(key_id) {
                Some(Secret::PrivateKey { pem, passphrase }) => (
                    Zeroizing::new(pem.clone()),
                    passphrase.as_ref().map(|p| Zeroizing::new(p.clone())),
                ),
                _ => return Err(Error::NotFound(key_id.clone())),
            };
            let private_pem = tokio::task::spawn_blocking(move || {
                decrypt_pem(&pem, passphrase.as_ref().map(|p| p.as_str()))
            })
            .await
            .map_err(|_| Error::InvalidInput("stored key could not be decrypted".into()))??;
            keys.push(ProvisionKey {
                name: meta.name.clone(),
                public_openssh: meta.public_key.clone(),
                private_pem,
            });
        }
    }

    let opts = ProvisionOptions {
        host: conn.host,
        port: conn.port,
        user: conn.user,
        password,
        known_hosts: state.paths.data_dir.join("known_hosts"),
        disable_password_auth,
    };
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(90),
        install_keys(&opts, &keys),
    )
    .await
    .map_err(|_| Error::Config("provisioning timed out".into()))??;
    Ok(Response::Provisioned(outcome))
}

/// Return an unencrypted OpenSSH PEM, decrypting with `passphrase` if given.
/// Errors never carry key material.
fn decrypt_pem(pem: &str, passphrase: Option<&str>) -> Result<Zeroizing<String>> {
    let Some(pass) = passphrase else {
        return Ok(Zeroizing::new(pem.to_string()));
    };
    let fail = |_| Error::InvalidInput("stored key could not be decrypted".into());
    ssh_key::PrivateKey::from_openssh(pem)
        .and_then(|k| k.decrypt(pass))
        .and_then(|k| k.to_openssh(ssh_key::LineEnding::LF))
        .map_err(fail)
}

/// Discover disk keys under `ssh_dir` (plus any `IdentityFile` in `~/.ssh/config`
/// pointing outside it), upsert their metadata, and return their views. Because
/// the metadata `id` is the fingerprint, repeat scans upsert rather than
/// duplicate.
pub fn sync_disk_keys(store: &Store, ssh_dir: &Path) -> Result<Vec<KeyView>> {
    let discovery = keys::discover_keys(ssh_dir)?;
    let config = std::fs::read_to_string(ssh_dir.join("config")).unwrap_or_default();
    let hosts = parse_ssh_config(&config);

    let mut keys = discovery.keys;
    let mut seen: std::collections::HashSet<String> =
        keys.iter().map(|k| k.fingerprint.clone()).collect();

    for host in hosts {
        let Some(path) = host.identity_file else {
            continue;
        };
        if !path.exists() || path.parent() == Some(ssh_dir) {
            continue;
        }
        let Ok(key) = keys::read_key(&path) else {
            continue;
        };
        if seen.insert(key.fingerprint.clone()) {
            keys.push(key);
        }
    }

    let mut views = Vec::with_capacity(keys.len());
    for key in keys {
        let name = key
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut meta = KeyMetadata::now(
            key.fingerprint.clone(),
            key.algorithm,
            key.public_key.clone(),
            key.fingerprint.clone(),
            KeySource::Disk,
        );
        meta.name = name;
        meta.disk_path = Some(key.path.display().to_string());
        meta.has_passphrase = key.has_passphrase;
        store.upsert_key(meta.clone())?;
        views.push(KeyView {
            key: meta,
            available: key.path.exists(),
        });
    }

    views.sort_by(|a, b| (&a.key.name, &a.key.fingerprint).cmp(&(&b.key.name, &b.key.fingerprint)));
    Ok(views)
}

/// Run the host until shutdown: serve the agent and the IPC socket.
pub async fn run(paths: Paths, socket: Option<PathBuf>) -> Result<()> {
    let addr = socket.unwrap_or_else(|| paths.socket_path());
    let mut state = HostState::new(paths.clone()).await?;
    // Advertise the socket this process really binds, not the derived default.
    state.ipc_addr = addr.display().to_string();
    let state = std::sync::Arc::new(state);
    let agent = crate::agent::serve(&paths.agent_socket(), state.agent.clone()).await?;
    let result = crate::ipc::server::serve(&addr, state.clone()).await;
    agent.abort();
    state.agent.clear();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decrypt_pem_round_trips_and_rejects_wrong_passphrase() {
        let g = keys::generate(KeyAlgorithm::Ed25519, Some("pw")).unwrap();
        let ok = decrypt_pem(&g.private_pem, Some("pw")).unwrap();
        assert!(ok.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
        assert!(ssh_key::PrivateKey::from_openssh(&*ok).is_ok_and(|k| !k.is_encrypted()));
        assert!(matches!(
            decrypt_pem(&g.private_pem, Some("wrong")),
            Err(Error::InvalidInput(_))
        ));
    }

    async fn state_in(dir: &Path) -> HostState {
        HostState::new(Paths::with_roots(dir.into(), dir.into(), dir.join("ssh")))
            .await
            .unwrap()
    }

    async fn provision(state: &HostState, id: &str, key_ids: &[&str]) -> Result<Response> {
        connections_provision(
            state,
            id.into(),
            "x".into(),
            key_ids.iter().map(|k| k.to_string()).collect(),
            false,
        )
        .await
    }

    async fn unlock(state: &HostState, dir: &Path) {
        let path = dir.join("test-vault");
        Vault::new(path.clone()).create("pw").unwrap();
        *state.vault.lock().await = Some(Vault::new(path).unlock("pw").unwrap());
    }

    #[tokio::test]
    async fn provisioning_without_any_key_is_invalid_input() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        state
            .store
            .upsert_connection(crate::model::Connection::managed("c1", "web"))
            .unwrap();
        match provision(&state, "c1", &[]).await {
            Err(Error::InvalidInput(m)) => assert_eq!(m, "no key selected"),
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn provisioning_a_disk_key_asks_for_import() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        state
            .store
            .upsert_connection(crate::model::Connection::managed("c1", "web"))
            .unwrap();
        state
            .store
            .upsert_key(KeyMetadata::now(
                "k1",
                KeyAlgorithm::Ed25519,
                "ssh-ed25519 AAAA",
                "SHA256:x",
                KeySource::Disk,
            ))
            .unwrap();
        unlock(&state, dir.path()).await;
        match provision(&state, "c1", &["k1"]).await {
            Err(Error::InvalidInput(m)) => {
                assert_eq!(m, "import the key into the vault to provision it")
            }
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn setting_and_clearing_a_connection_password() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::with_roots(dir.path().into(), dir.path().into(), dir.path().into());
        let state = HostState::new(paths).await.unwrap();
        // unlock
        handle(
            &state,
            Request::VaultInit {
                passphrase: "pw".into(),
            },
        )
        .await;
        handle(
            &state,
            Request::VaultUnlock {
                passphrase: "pw".into(),
            },
        )
        .await;
        let mut c = Connection::managed("c1", "web");
        c.host = "example.com".into();
        c.user = "deploy".into();
        assert!(matches!(
            handle(&state, Request::ConnectionsAdd { connection: c }).await,
            Response::Ok
        ));
        assert!(matches!(
            handle(
                &state,
                Request::ConnectionsSetPassword {
                    id: "c1".into(),
                    password: Some("s3cret".into())
                }
            )
            .await,
            Response::Ok
        ));
        // listing reports it without leaking the value
        match handle(&state, Request::ConnectionsList).await {
            Response::Connections(cs) => assert!(cs[0].has_password),
            other => panic!("{other:?}"),
        }
        // the secret is readable only through the dedicated request
        match handle(&state, Request::PasswordGet { id: "c1".into() }).await {
            Response::Password(p) => assert_eq!(p, "s3cret"),
            other => panic!("{other:?}"),
        }
        // clearing
        handle(
            &state,
            Request::ConnectionsSetPassword {
                id: "c1".into(),
                password: None,
            },
        )
        .await;
        assert!(matches!(
            handle(&state, Request::PasswordGet { id: "c1".into() }).await,
            Response::Error { .. }
        ));
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn status_reports_the_bound_socket() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = state_in(dir.path()).await;
        // `sigrun host --socket …` overrides the derived default; the status is
        // what the CLI reports and the askpass prefix is built from the same
        // field, so both name the socket the host is really serving on.
        state.ipc_addr = "/tmp/sigrun-bound.sock".into();
        match handle(&state, Request::Status).await {
            Response::Status(info) => assert_eq!(info.host_addr, "/tmp/sigrun-bound.sock"),
            other => panic!("{other:?}"),
        }
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn connecting_with_a_stored_password_limits_the_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        unlock(&state, dir.path()).await;
        let mut c = Connection::managed("c1", "web");
        c.host = "example.com".into();
        state.store.upsert_connection(c).unwrap();

        connections_set_password(&state, "c1".into(), Some("s3cret".into()))
            .await
            .unwrap();
        // `print_only` builds the same argv without contacting the host.
        let Response::Command(cmd) = connections_connect(&state, "c1".into(), true, false)
            .await
            .unwrap()
        else {
            panic!("expected a command");
        };
        assert!(cmd.contains("NumberOfPasswordPrompts=1"), "{cmd}");

        connections_set_password(&state, "c1".into(), None)
            .await
            .unwrap();
        let Response::Command(cmd) = connections_connect(&state, "c1".into(), true, false)
            .await
            .unwrap()
        else {
            panic!("expected a command");
        };
        assert!(!cmd.contains("NumberOfPasswordPrompts"), "{cmd}");
        state.shutdown.cancel();
    }

    #[test]
    fn has_password_survives_the_ipc_wire_round_trip() {
        let mut conn = Connection::managed("c1", "web");
        conn.has_password = true;
        let wire =
            crate::ipc::protocol::encode(&Response::Connections(vec![conn])).expect("encode");
        match crate::ipc::protocol::decode::<Response>(&wire) {
            Ok(Response::Connections(cs)) => assert!(cs[0].has_password),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_client_supplied_password_flag_is_not_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        let mut c = Connection::managed("c1", "web");
        c.has_password = true;
        assert!(matches!(
            handle(&state, Request::ConnectionsAdd { connection: c }).await,
            Response::Ok
        ));
        let stored = state.store.load_data().unwrap().connections;
        assert!(!stored[0].has_password);
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn a_locked_vault_refuses_to_store_a_password() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        handle(
            &state,
            Request::VaultInit {
                passphrase: "pw".into(),
            },
        )
        .await;
        // VaultInit but no VaultUnlock -> ConnectionsSetPassword is vault_locked
        match handle(
            &state,
            Request::ConnectionsSetPassword {
                id: "c1".into(),
                password: Some("s3cret".into()),
            },
        )
        .await
        {
            Response::Error { code, .. } => assert_eq!(code, "vault_locked"),
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn removing_a_connection_removes_its_password() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        handle(
            &state,
            Request::VaultInit {
                passphrase: "pw".into(),
            },
        )
        .await;
        handle(
            &state,
            Request::VaultUnlock {
                passphrase: "pw".into(),
            },
        )
        .await;
        handle(
            &state,
            Request::ConnectionsAdd {
                connection: Connection::managed("c1", "web"),
            },
        )
        .await;
        handle(
            &state,
            Request::ConnectionsSetPassword {
                id: "c1".into(),
                password: Some("s3cret".into()),
            },
        )
        .await;
        assert!(matches!(
            handle(&state, Request::ConnectionsRemove { id: "c1".into() }).await,
            Response::Ok
        ));
        // after ConnectionsRemove, PasswordGet for that id is not_found
        match handle(&state, Request::PasswordGet { id: "c1".into() }).await {
            Response::Error { code, .. } => assert_eq!(code, "not_found"),
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn provisioning_an_ambiguous_label_lists_the_ids() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        for id in ["c1", "c2"] {
            state
                .store
                .upsert_connection(crate::model::Connection::managed(id, "web"))
                .unwrap();
        }
        match provision(&state, "web", &["k1"]).await {
            Err(Error::InvalidInput(m)) => assert!(m.contains("c1") && m.contains("c2")),
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }
}
