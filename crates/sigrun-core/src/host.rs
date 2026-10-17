//! The headless Sigrun host: owns the vault, the agent, and the TOML store,
//! and serves every CLI request over the local IPC socket.

use std::path::{Path, PathBuf};

use crate::agent::{vault_keys, AgentState};
use crate::error::{Error, Result};
use crate::ipc::protocol::{error_code, Request, Response, StatusInfo};
use crate::keys::{self, GeneratedKey, ImportedKey};
use crate::model::{KeyAlgorithm, KeyMetadata, KeySource, KeyView};
use crate::paths::{ensure_private_dir, Paths};
use crate::ssh_config::parse_ssh_config;
use crate::store::Store;
use crate::terminal::{
    launch_first, render_command, ssh_invocation, terminal_launch_specs, Platform,
};
use crate::vault::{Secret, UnlockedVault, Vault, VaultStatus};

/// Long-lived host state. The lock-file handle stays open for the host's
/// lifetime so no second host can start against the same data directory.
pub struct HostState {
    pub paths: Paths,
    pub store: Store,
    pub vault: tokio::sync::Mutex<Option<UnlockedVault>>,
    pub agent: AgentState,
    pub agent_socket: String,
    pub host_addr: String,
    pub shutdown: tokio_util::sync::CancellationToken,
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
            .open(paths.lock_file())?;
        fs2::FileExt::try_lock_exclusive(&lock_file).map_err(|_| Error::AlreadyRunning)?;

        Ok(HostState {
            agent_socket: paths.agent_socket().display().to_string(),
            host_addr: paths.socket_path().display().to_string(),
            store: Store::new(paths.clone()),
            vault: tokio::sync::Mutex::new(None),
            agent: AgentState::new(),
            shutdown: tokio_util::sync::CancellationToken::new(),
            paths,
            lock_file,
        })
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
    match req {
        Request::Status => {
            let vault = vault_status(state).await;
            Ok(Response::Status(StatusInfo {
                version: crate::VERSION.to_string(),
                vault,
                agent_socket: Some(state.agent_socket.clone()),
                host_addr: state.host_addr.clone(),
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
        Request::ConnectionsList => Ok(Response::Connections(state.store.load_data()?.connections)),
        Request::ConnectionsAdd { mut connection } => {
            if connection.id.trim().is_empty() {
                connection.id = uuid::Uuid::new_v4().to_string();
            }
            state.store.upsert_connection(connection)?;
            Ok(Response::Ok)
        }
        Request::ConnectionsUpdate { connection } => {
            state.store.upsert_connection(connection)?;
            Ok(Response::Ok)
        }
        Request::ConnectionsRemove { id } => {
            state.store.remove_connection(&id)?;
            Ok(Response::Ok)
        }
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
        Request::ConnectionsConnect { id, print_only } => {
            connections_connect(state, id, print_only).await
        }
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

async fn connections_connect(state: &HostState, id: String, print_only: bool) -> Result<Response> {
    let data = state.store.load_data()?;
    let conn = data
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

    let argv = ssh_invocation(&conn, meta.as_ref())?;
    if print_only {
        return Ok(Response::Command(render_command(&argv)));
    }

    let specs = terminal_launch_specs(Platform::current(), settings.terminal.as_deref(), &argv)?;
    launch_first(&specs, Some(&state.paths.agent_socket()))?;
    Ok(Response::Ok)
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
    let state = std::sync::Arc::new(HostState::new(paths.clone()).await?);
    let agent = crate::agent::serve(&paths.agent_socket(), state.agent.clone()).await?;
    let addr = socket.unwrap_or_else(|| paths.socket_path());
    let result = crate::ipc::server::serve(&addr, state.clone()).await;
    agent.abort();
    state.agent.clear();
    result
}
