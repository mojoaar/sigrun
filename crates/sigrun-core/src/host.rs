//! The headless Sigrun host: owns the vault, the agent, and the TOML store,
//! and serves every CLI request over the local IPC socket.

use std::path::{Path, PathBuf};

use crate::agent::{vault_keys, AgentState};
use crate::bundle::{self, ExportSummary, ImportSummary};
use crate::error::{Error, Result};
use crate::ipc::protocol::{error_code, Request, Response, StatusInfo};
use crate::keys::{self, GeneratedKey, ImportedKey};
use crate::model::{
    effective, Auth, Connection, Credential, DataFile, KeyAlgorithm, KeyMetadata, KeySource,
    KeyView,
};
use crate::paths::{ensure_private_dir, Paths};
use crate::ssh_config::parse_ssh_config;
use crate::store::Store;
use crate::terminal::{
    agent_prefix, env_prefix, launch_first, launch_reports, probe_reachable, render_command,
    ssh_invocation, terminal_launch_specs, Platform,
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
    /// Ids with a live session, reported as `Connection.active`. Transient:
    /// never persisted, and dropped when the vault locks.
    pub active: std::sync::Mutex<std::collections::HashSet<String>>,
    /// The launched shell's pid per connection id, so a session whose terminal
    /// killed the whole process group can still be swept. In memory only: never
    /// persisted, and dropped when the vault locks with `active`.
    pub sessions: std::sync::Mutex<std::collections::HashMap<String, u32>>,
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
            active: std::sync::Mutex::new(std::collections::HashSet::new()),
            sessions: std::sync::Mutex::new(std::collections::HashMap::new()),
            paths,
            lock_file,
        })
    }

    /// The active set. A poisoned lock means some handler panicked while
    /// holding it; recovering keeps that from panicking every later request.
    fn active_set(&self) -> std::sync::MutexGuard<'_, std::collections::HashSet<String>> {
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn mark_active(&self, id: &str) {
        self.active_set().insert(id.to_string());
    }

    pub fn clear_active(&self, id: &str) {
        self.active_set().remove(id);
    }

    pub fn clear_all_active(&self) {
        self.active_set().clear();
    }

    pub fn is_active(&self, id: &str) -> bool {
        self.active_set().contains(id)
    }

    /// Sorted, so callers can compare snapshots with `!=`.
    pub fn active_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.active_set().iter().cloned().collect();
        ids.sort();
        ids
    }

    /// The watched pids. Same poisoning recovery as [`Self::active_set`].
    fn session_map(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<String, u32>> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Remember the launched shell that owns `id`'s session. A second report
    /// overwrites the first, so the session lasts as long as the last shell.
    pub fn report_session(&self, id: &str, pid: u32) {
        self.session_map().insert(id.to_string(), pid);
    }

    /// Forget the watched shell for `id`, alongside [`Self::clear_active`].
    pub fn forget_session(&self, id: &str) {
        self.session_map().remove(id);
    }

    /// Clears every connection whose watched shell is gone. True when the active
    /// set changed, so the caller can emit. Only ever clears, so a late report
    /// can never resurrect a session that a sweep already ended.
    ///
    /// A pid recycled inside the poll window reads as alive and is accepted: the
    /// next sweep of the reaped process catches it.
    pub fn sweep_dead_sessions(&self) -> bool {
        let mut changed = false;
        self.session_map().retain(|id, pid| {
            if pid_is_alive(*pid) && !session_is_orphaned(*pid) {
                return true;
            }
            // A shell reparented to init lost the terminal that launched it, but
            // no hangup reached it, so it would sit there with an unreachable
            // ssh under it for good. Signal its group: the trap reports the end.
            end_orphaned_session(*pid);
            if self.is_active(id) {
                self.clear_active(id);
                changed = true;
            }
            false
        });
        changed
    }

    /// Drop everything that only exists while the vault is unlocked.
    async fn lock_vault(&self) {
        *self.vault.lock().await = None;
        self.agent.clear();
        self.clear_all_active();
        self.session_map().clear();
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
                self.lock_vault().await;
                self.auto_locked.store(true, Ordering::Relaxed);
            }
        }
    }

    /// Write every connection and every key's public metadata to `path`, as a
    /// folder or a `.zip`. Reads only `data.toml`, so the vault stays as it is.
    async fn export_bundle(&self, path: &str, zipped: bool) -> Result<ExportSummary> {
        let dest = Path::new(path);
        if !bundle::destination_is_allowed(dest, &self.paths.ssh_dir) {
            return Err(Error::InvalidInput(
                "cannot export inside the SSH directory".into(),
            ));
        }
        // Never half-overwrite: a destination that already holds either root
        // entry, or an existing zip file, is refused rather than clobbered.
        let taken = if zipped {
            dest.exists()
        } else {
            [bundle::EXPORT_FILE, bundle::README_FILE]
                .iter()
                .any(|name| dest.join(name).exists())
        };
        if taken {
            return Err(Error::AlreadyExists(path.to_string()));
        }

        let data = self.store.load_data()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let built = bundle::build_bundle(&data, now);
        if zipped {
            bundle::write_zip(&built, dest)?;
        } else {
            bundle::write_folder(&built, dest)?;
        }
        Ok(ExportSummary {
            path: path.to_string(),
            connections: built.connections.len(),
            keys: built.keys.len(),
        })
    }

    /// Merge the connections of the bundle at `path` into the store. Nothing is
    /// overwritten or deleted, and the vault is never read or written.
    async fn import_bundle(&self, path: &str) -> Result<ImportSummary> {
        let src = Path::new(path);
        if !src.exists() {
            return Err(Error::NotFound(path.to_string()));
        }
        let incoming = bundle::read_bundle(src)?;
        // Always through `merge`: what lands in the store is rebuilt from
        // connections the launcher accepts, so a hostile `export.json` never
        // survives into a later export verbatim.
        let mut summary = ImportSummary::default();
        self.store.update_data(|data| {
            summary = bundle::merge(data, &incoming);
            Ok(())
        })?;
        Ok(summary)
    }
}

/// Whether `pid` names a live process. `kill(pid, 0)` sends no signal; it only
/// checks existence and permission. Any errno other than `ESRCH` counts as
/// alive, so a permission quirk never clears a live session.
#[cfg(unix)]
fn pid_is_alive(pid: u32) -> bool {
    // SAFETY: signal 0 performs only the existence and permission check.
    if unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// Nothing is ever swept where there is no way to ask: Windows launches through
/// `cmd.exe /k` and reports no pid at all.
#[cfg(not(unix))]
fn pid_is_alive(_pid: u32) -> bool {
    true
}

/// The parent pid of `pid`, when the platform can be asked.
#[cfg(target_os = "macos")]
fn parent_of(pid: u32) -> Option<u32> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: `info` is a zeroed buffer of exactly the size we pass, and the
    // call fills it in when it recognises the flavour.
    let read = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            std::ptr::addr_of_mut!(info).cast(),
            size,
        )
    };
    (read == size).then_some(info.pbi_ppid)
}

#[cfg(target_os = "linux")]
fn parent_of(pid: u32) -> Option<u32> {
    // Field 4 of /proc/<pid>/stat, after the command name in parentheses.
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn parent_of(_pid: u32) -> Option<u32> {
    None
}

/// Whether the shell that reported this session has been reparented to init,
/// meaning the terminal that launched it is gone but sent no hangup.
#[cfg(unix)]
fn session_is_orphaned(pid: u32) -> bool {
    matches!(parent_of(pid), Some(parent) if parent <= 1)
}

#[cfg(not(unix))]
fn session_is_orphaned(_pid: u32) -> bool {
    false
}

/// End a session whose terminal is gone but whose shell never got a hangup.
#[cfg(unix)]
fn end_orphaned_session(pid: u32) {
    // SAFETY: the pid came from our own launch line, which makes it a session
    // leader; a negative pid targets its process group so the ssh under it goes
    // too. The trap that runs on SIGHUP reports the end over IPC.
    unsafe {
        libc::kill(-(pid as libc::pid_t), libc::SIGHUP);
    }
}

#[cfg(not(unix))]
fn end_orphaned_session(_pid: u32) {}

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
            | Request::KeysExport { .. }
            | Request::ConnectionsConnect { .. }
            | Request::ConnectionsProvision { .. }
            | Request::ConnectionsSetPassword { .. }
            | Request::CredentialsSetPassword { .. }
            | Request::CredentialsRemove { .. }
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
            state.lock_vault().await;
            Ok(Response::Ok)
        }
        Request::ConnectionsList => {
            let mut connections = state.store.load_data()?.connections;
            fill_password_flags(state, &mut connections).await;
            fill_active_flags(state, &mut connections);
            Ok(Response::Connections(connections))
        }
        Request::ConnectionsAdd { mut connection } => {
            if connection.id.trim().is_empty() {
                connection.id = uuid::Uuid::new_v4().to_string();
            }
            // `has_password` is host-derived. Clearing it keeps anything a
            // client sends out of `data.toml`; readers fill it from the vault.
            connection.has_password = false;
            // `active` is host-derived too: a client cannot claim a session.
            connection.active = false;
            check_identity(&state.store.load_data()?, &connection)?;
            state.store.upsert_connection(connection)?;
            Ok(Response::Ok)
        }
        Request::ConnectionsUpdate { mut connection } => {
            connection.has_password = false;
            connection.active = false;
            let data = state.store.load_data()?;
            check_identity(&data, &connection)?;
            refuse_saved_password(state, &data, &connection).await?;
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
            state.clear_active(&id);
            state.forget_session(&id);
            Ok(Response::Ok)
        }
        Request::ConnectionsSetPassword { id, password } => {
            connections_set_password(state, id, password).await
        }
        Request::CredentialsSetPassword { id, password } => {
            credentials_set_password(state, id, password).await
        }
        // Add and update write metadata only; remove also clears the secret.
        Request::CredentialsAdd {
            credential,
            password,
        } => credentials_add(state, credential, password).await,
        Request::CredentialsUpdate { credential } => credentials_update(state, credential).await,
        Request::CredentialsRemove { id } => credentials_remove(state, id).await,
        Request::PasswordGet { id } => password_get(state, id).await,
        Request::KeysList => keys_list(state).await,
        Request::CredentialsList => credentials_list(state).await,
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
        Request::KeysExport { id, path } => keys_export(state, id, path).await,
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
        // Not vault-touching: an export or an import reads only `data.toml`, so
        // neither may defer the idle auto-lock.
        Request::ExportBundle { path, zipped } => state
            .export_bundle(&path, zipped)
            .await
            .map(Response::Export),
        Request::ImportBundle { path } => state.import_bundle(&path).await.map(Response::Import),
        // Not vault-touching: a session can end long after the launch that
        // bumped the clock, and that must not defer the idle auto-lock.
        Request::ConnectionEnded { id } => {
            state.clear_active(&id);
            Ok(Response::Ok)
        }
        // Not vault-touching either: the report arrives right after a launch the
        // connect already bumped, and it must not defer the idle auto-lock.
        Request::SessionStarted { id, pid } => {
            state.report_session(&id, pid);
            Ok(Response::Ok)
        }
        Request::Shutdown => {
            state.shutdown.cancel();
            Ok(Response::Ok)
        }
    }
}

/// The credential `id` names, if it is stored.
fn credential_by_id<'a>(data: &'a DataFile, id: &str) -> Option<&'a Credential> {
    data.credentials.iter().find(|cred| cred.id == id)
}

/// Refuse a connection whose stored shape cannot authenticate: a credential
/// mixed with the connection's own identity, or a keyless credential with
/// password auth turned off. The caller's fields are refused, never repaired.
///
/// This is the backstop for callers that skip [`bundle::valid_connection`].
fn check_identity(data: &DataFile, conn: &Connection) -> Result<()> {
    let Some(cred_id) = conn.credential_id.as_deref() else {
        return Ok(());
    };
    // The stored-secret refusal in `refuse_saved_password` is what really
    // enforces the password side; the host clears this flag on the way in.
    if !conn.user.is_empty() || conn.key_id.is_some() || conn.has_password {
        return Err(Error::InvalidInput(
            "a connection with a credential cannot also set its own user, key or password".into(),
        ));
    }
    match credential_by_id(data, cred_id) {
        Some(cred) if conn.disable_password_auth && cred.key_id.is_none() => {
            Err(Error::InvalidInput(format!(
                "credential {cred_id} has no key, so password authentication cannot be disabled"
            )))
        }
        _ => Ok(()),
    }
}

/// Refuse adopting a credential while the connection still holds its own
/// password: the spec allows one source only, and removing a secret is the
/// user's call, never a side effect of a save.
///
/// A locked vault cannot see the secret, so a first switch to a credential is
/// refused rather than guessed: the alternative would store a row whose
/// password nobody can reach any more.
async fn refuse_saved_password(
    state: &HostState,
    data: &DataFile,
    conn: &Connection,
) -> Result<()> {
    if conn.credential_id.as_deref().is_none_or(str::is_empty) {
        return Ok(());
    }
    let stored = data.connections.iter().find(|c| c.id == conn.id);
    let already_shared = stored
        .and_then(|c| c.credential_id.as_deref())
        .is_some_and(|id| !id.is_empty());
    let stored_flag = stored.is_some_and(|c| c.has_password);

    let guard = state.vault.lock().await;
    let vault = (*guard).as_ref();
    if !already_shared && vault.is_none() {
        return Err(Error::VaultLockedFor(
            "unlock the vault to switch this connection to a credential".into(),
        ));
    }
    let mut probe = conn.clone();
    with_password_flag(vault, &mut probe);
    if !stored_flag && !probe.has_password {
        return Ok(());
    }
    Err(Error::InvalidInput(format!(
        "connection {} still has a saved password; remove it before using a credential",
        conn.label
    )))
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

/// The list works while locked: `has_password` is stored metadata, not derived
/// from the vault.
async fn credentials_list(state: &HostState) -> Result<Response> {
    let data = state.store.load_data()?;
    Ok(Response::Credentials(data.credentials))
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

/// Write a vault key's private half to disk, mode `0600`. Refused inside
/// `~/.ssh`, where Sigrun only reads, and when the file already exists.
async fn keys_export(state: &HostState, id: String, path: String) -> Result<Response> {
    let data = state.store.load_data()?;
    let meta = data
        .keys
        .iter()
        .find(|k| k.id == id)
        .cloned()
        .ok_or_else(|| Error::NotFound(id.clone()))?;
    if meta.source != KeySource::Vault {
        return Err(Error::InvalidInput(format!(
            "key {} is on disk already",
            meta.name
        )));
    }

    let dest = std::path::PathBuf::from(&path);
    if !bundle::destination_is_allowed(&dest, &state.paths.ssh_dir) {
        return Err(Error::InvalidInput(
            "a key cannot be written into ~/.ssh".into(),
        ));
    }

    let pem = {
        let mut guard = state.vault.lock().await;
        let vault = (*guard).as_mut().ok_or(Error::VaultLocked)?;
        match vault.get(&id) {
            Some(Secret::PrivateKey { pem, .. }) => pem.clone(),
            _ => return Err(Error::NotFound(id.clone())),
        }
    };

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&dest).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => Error::AlreadyExists(path.clone()),
        _ => Error::Io(e),
    })?;
    std::io::Write::write_all(&mut file, pem.as_bytes())?;
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

/// Whether the unlocked vault holds a password under `key`. A locked vault
/// answers false, so ssh prompts instead of asking a helper that cannot reply.
async fn vault_has_password(state: &HostState, key: Option<&str>) -> bool {
    let Some(key) = key else {
        return false;
    };
    let guard = state.vault.lock().await;
    (*guard)
        .as_ref()
        .is_some_and(|v| matches!(v.get(key), Some(Secret::Password { .. })))
}

/// Fill `active` for each connection from the host's live-session set.
fn fill_active_flags(state: &HostState, connections: &mut [Connection]) {
    for conn in connections {
        conn.active = state.is_active(&conn.id);
    }
}

/// The askpass prefix for an identity that the vault can serve a password for,
/// or `None` when it cannot. A separate function so the decision is testable
/// without driving a real terminal launch.
fn askpass_prefix(auth: &Auth, socket: &Path, connection_id: &str) -> Option<String> {
    // The host may serve on `--socket`, so advertise the bound address.
    auth.password_key
        .is_some()
        .then(|| env_prefix(socket, connection_id))
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

/// Store or clear a credential's password under its id, flipping `has_password`
/// in the same update. `None` clears; an absent entry is a silent no-op. The
/// value is never logged or echoed in an error.
async fn credentials_set_password(
    state: &HostState,
    id: String,
    password: Option<String>,
) -> Result<Response> {
    let mut guard = state.vault.lock().await;
    let vault = (*guard).as_mut().ok_or(Error::VaultLocked)?;
    let data = state.store.load_data()?;
    let stored = data
        .credentials
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| Error::NotFound(id.clone()))?;
    // A keyless credential is a password-only identity. Clearing that last
    // secret would leave nothing to authenticate with, so the row must go.
    if password.is_none() && stored.key_id.is_none() {
        return Err(Error::InvalidInput(format!(
            "credential '{}' has no key, so its password cannot be removed; delete the credential instead",
            stored.label
        )));
    }
    let has_password = match password {
        Some(password) => {
            if password.is_empty() {
                return Err(Error::InvalidInput("password must not be empty".into()));
            }
            vault.put(&id, Secret::Password { password })?;
            true
        }
        None => {
            vault.remove(&id)?;
            false
        }
    };
    state.store.update_data(|data| {
        if let Some(cred) = data.credentials.iter_mut().find(|c| c.id == id) {
            cred.has_password = has_password;
        }
        Ok(())
    })?;
    Ok(Response::Ok)
}

/// The single refusal for a credential a caller may not store. The rules live
/// in [`bundle::credential_valid`]; this only turns a `false` into an English
/// error, so the CLI, the app and an import cannot drift apart.
fn check_credential(credential: &Credential, all: &[Credential]) -> Result<()> {
    if bundle::credential_valid(credential, all) {
        return Ok(());
    }
    Err(Error::InvalidInput(format!(
        "invalid credential '{}': a label, a user, a key or a password, and a label no other credential uses are required",
        credential.label
    )))
}

/// Create a credential. The host assigns the id, and a password is sealed in
/// the same metadata transaction, so `has_password` can never disagree with the
/// vault. Integration and connection ids share the vault keyspace, so a caller
/// may not choose an id.
async fn credentials_add(
    state: &HostState,
    mut credential: Credential,
    password: Option<String>,
) -> Result<Response> {
    if !credential.id.trim().is_empty() {
        return Err(Error::InvalidInput(
            "credential ids are assigned by the host".into(),
        ));
    }
    if password.as_deref() == Some("") {
        return Err(Error::InvalidInput("password must not be empty".into()));
    }
    credential.id = uuid::Uuid::new_v4().to_string();
    // Never trust the payload's flag: the password argument decides.
    credential.has_password = password.is_some();
    let data = state.store.load_data()?;
    check_credential(&credential, &data.credentials)?;

    // Hold the vault for the whole add when a secret is written: a locked
    // vault refuses before anything is stored, and the secret cannot lag the
    // metadata behind.
    let mut guard = state.vault.lock().await;
    let vault = match &password {
        Some(_) => Some((*guard).as_mut().ok_or(Error::VaultLocked)?),
        None => None,
    };
    let id = credential.id.clone();
    state.store.update_data(|data| {
        data.credentials.push(credential);
        if let (Some(vault), Some(password)) = (vault, password) {
            vault.put(&id, Secret::Password { password })?;
        }
        Ok(())
    })?;
    Ok(Response::Ok)
}

/// Replace a credential's metadata. The id is the row's, the password is not
/// touched, and the stored flag wins: otherwise a caller could clear it while
/// the vault still holds the secret.
async fn credentials_update(state: &HostState, mut credential: Credential) -> Result<Response> {
    let data = state.store.load_data()?;
    let stored = data
        .credentials
        .iter()
        .find(|c| c.id == credential.id)
        .cloned()
        .ok_or_else(|| Error::NotFound(format!("credential {}", credential.id)))?;
    credential.has_password = stored.has_password;
    check_credential(&credential, &data.credentials)?;
    state.store.update_data(|data| {
        if let Some(row) = data.credentials.iter_mut().find(|c| c.id == credential.id) {
            *row = credential;
        }
        Ok(())
    })?;
    Ok(Response::Ok)
}

/// Delete a credential and its secret. A connection that still references it
/// refuses the delete and names the count: silently clearing the reference
/// would turn a working connection into a different identity.
async fn credentials_remove(state: &HostState, id: String) -> Result<Response> {
    let data = state.store.load_data()?;
    if !data.credentials.iter().any(|c| c.id == id) {
        return Err(Error::NotFound(format!("credential {id}")));
    }
    let used_by = data
        .connections
        .iter()
        .filter(|conn| conn.credential_id.as_deref() == Some(id.as_str()))
        .count();
    if used_by > 0 {
        return Err(Error::InvalidInput(format!(
            "credential {id} is used by {used_by} connections"
        )));
    }
    // Clear the secret while the vault is open. Like a connection's password, a
    // locked vault must not block removing metadata nobody can use.
    {
        let mut guard = state.vault.lock().await;
        if let Some(vault) = (*guard).as_mut() {
            vault.remove(&id)?;
        }
    }
    state.store.update_data(|data| {
        data.credentials.retain(|c| c.id != id);
        Ok(())
    })?;
    Ok(Response::Ok)
}

/// Hand a connection's stored password to the caller — the askpass helper. The
/// connection is resolved through [`effective`], so a shared credential serves
/// every connection that references it. The only request that ever returns a
/// secret; it travels to the 0600 socket exactly as `VaultUnlock`'s passphrase.
async fn password_get(state: &HostState, id: String) -> Result<Response> {
    let data = state.store.load_data()?;
    let mut conn = data
        .connections
        .iter()
        .find(|c| c.id == id)
        .cloned()
        .ok_or_else(|| Error::NotFound(id.clone()))?;
    let guard = state.vault.lock().await;
    let vault = (*guard).as_ref().ok_or(Error::VaultLocked)?;
    // An own connection's flag is host-derived and never persisted, so fill it
    // from the vault before resolving; a credential connection ignores it.
    conn.has_password = matches!(vault.get(&conn.id), Some(Secret::Password { .. }));
    let auth = effective(&conn, &data.credentials)?;
    let key = auth
        .password_key
        .ok_or_else(|| Error::NotFound(id.clone()))?;
    match vault.get(&key) {
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

    // The store never persists `has_password`, so an own connection's flag has
    // to come from the vault before its identity can be resolved.
    conn.has_password = vault_has_password(state, Some(&conn.id)).await;
    let mut auth = effective(&conn, &data.credentials)?;

    let meta = match &auth.key_id {
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
    // host get contacted, so a bad key never costs a probe round trip. A probe
    // that cannot prove the host is out of reach only advises: ssh decides,
    // because a firewall can report EHOSTUNREACH for a host that answers.
    if !print_only && !force {
        if let Err(failure) =
            probe_reachable(&conn.host, conn.port, std::time::Duration::from_secs(3)).await
        {
            if failure.blocks() {
                return Err(failure.into_error());
            }
            eprintln!(
                "sigrun: probe inconclusive, letting ssh try: {}",
                failure.message()
            );
        }
    }

    // Only a password the vault can actually serve drives the ssh argv and the
    // askpass prefix. A locked vault cannot answer, so the connection stays
    // password-less and ssh prompts as before.
    if !vault_has_password(state, auth.password_key.as_deref()).await {
        auth.password_key = None;
    }

    let argv = ssh_invocation(&conn, &auth, meta.as_ref())?;
    if print_only {
        return Ok(Response::Command(render_command(&argv)));
    }

    // The agent socket and the askpass helper both have to travel inside the
    // command string: an AppleScript launch starts its own login shell, which
    // inherits nothing from this process's environment.
    let prefix = format!(
        "{}{}",
        agent_prefix(&state.paths.agent_socket()),
        askpass_prefix(&auth, Path::new(&state.ipc_addr), &conn.id).unwrap_or_default()
    );

    // Windows keeps `cmd.exe /k` with no way to chain a report, so the flag is
    // only set where the launched line can clear it again.
    let reports = launch_reports(Platform::current(), Path::new(&state.ipc_addr), &conn.id);
    let specs = terminal_launch_specs(
        Platform::current(),
        settings.terminal.as_deref(),
        &argv,
        &prefix,
        reports.as_ref(),
        settings.open_in.as_str(),
    )?;
    launch_first(&specs, Some(&state.paths.agent_socket()))?;
    if reports.is_some() {
        state.mark_active(&conn.id);
    }
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

    let auth = effective(&conn, &data.credentials)?;
    let key_ids = if key_ids.is_empty() {
        auth.key_id.clone().into_iter().collect()
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

    // An empty password means "use the one already saved": provisioning logs in
    // with the server account password, and a connection or credential may hold
    // it in the vault already. Never read it back to the UI — only used here.
    let password = if password.is_empty() {
        // `has_password` is host-filled when a list is rendered, so a row read back
        // from the store says false; a connection with no credential is therefore
        // asked for its own secret by id, not through the resolved identity.
        let saved_key = auth.password_key.clone().or_else(|| {
            conn.credential_id
                .as_deref()
                .filter(|id| !id.is_empty())
                .is_none()
                .then(|| conn.id.clone())
        });
        let guard = state.vault.lock().await;
        let vault = (*guard).as_ref().ok_or(Error::VaultLocked)?;
        match saved_key.as_deref().and_then(|key| vault.get(key)) {
            Some(Secret::Password { password }) => password.clone(),
            _ => {
                return Err(Error::InvalidInput(
                    "no password given and none saved for this connection".into(),
                ))
            }
        }
    } else {
        password
    };

    let opts = ProvisionOptions {
        host: conn.host,
        port: conn.port,
        // The identity is the resolved one; the password is the one the caller
        // supplied for this run, or the saved one when they supplied none.
        user: auth.user,
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

    /// Unlock a vault created where `VaultUnlock` looks, so the tests below can
    /// lock and unlock it over the IPC.
    async fn unlock_at_the_host_path(state: &HostState) {
        let path = state.paths.vault_file().clone();
        Vault::new(path.clone()).create("pw").unwrap();
        *state.vault.lock().await = Some(Vault::new(path).unlock("pw").unwrap());
    }

    /// A connection the launcher would accept: a real host and port.
    fn usable(id: &str, host: &str) -> Connection {
        let mut conn = Connection::managed(id, id);
        conn.host = host.into();
        conn.user = "me".into();
        conn
    }

    /// A host in its own tempdir with one connection already stored through the
    /// IPC, the way the app seeds one.
    async fn host_with_one_connection(dir: &Path) -> HostState {
        let state = state_in(dir).await;
        assert!(matches!(
            handle(
                &state,
                Request::ConnectionsAdd {
                    connection: usable("c1", "one.example"),
                }
            )
            .await,
            Response::Ok
        ));
        state
    }

    /// Generate a vault key and hand back its id.
    async fn vault_key(state: &HostState) -> String {
        match keys_generate(state, "seed".into(), KeyAlgorithm::Ed25519, None)
            .await
            .unwrap()
        {
            Response::Key(view) => view.key.id,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_vault_key_can_be_written_to_disk_but_not_into_ssh() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        unlock(&state, dir.path()).await;
        let key_id = vault_key(&state).await;

        let out = dir.path().join("copy.pem");
        keys_export(&state, key_id.clone(), out.to_string_lossy().into_owned())
            .await
            .unwrap();
        let written = std::fs::read_to_string(&out).unwrap();
        assert!(
            written.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"),
            "{written}"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&out).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        // A second write never clobbers the first.
        assert!(matches!(
            keys_export(&state, key_id.clone(), out.to_string_lossy().into_owned()).await,
            Err(Error::AlreadyExists(_))
        ));

        // `~/.ssh` is read-only for Sigrun, however the path is spelled.
        let inside = state.paths.ssh_dir.join("exported");
        std::fs::create_dir_all(&state.paths.ssh_dir).unwrap();
        assert!(matches!(
            keys_export(
                &state,
                key_id.clone(),
                inside.to_string_lossy().into_owned()
            )
            .await,
            Err(Error::InvalidInput(_))
        ));
        assert!(!inside.exists());
    }

    #[tokio::test]
    async fn provisioning_without_a_password_uses_the_saved_one() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        unlock(&state, dir.path()).await;
        let key_id = vault_key(&state).await;

        let mut conn = usable("c1", "127.0.0.1");
        conn.port = 1;
        conn.has_password = true;
        state.store.upsert_connection(conn.clone()).unwrap();
        state
            .vault
            .lock()
            .await
            .as_mut()
            .unwrap()
            .put(
                &conn.id,
                Secret::Password {
                    password: "server-pw".into(),
                },
            )
            .unwrap();

        // Nobody is listening on port 1, so reaching ssh at all proves the saved
        // password was used instead of being reported missing.
        match connections_provision(&state, "c1".into(), String::new(), vec![key_id], false).await {
            Err(Error::Unreachable(m)) => {
                assert!(m.contains("127.0.0.1:1"), "message was {m:?}");
            }
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn provisioning_reads_a_connection_password_the_stored_row_does_not_flag() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        unlock(&state, dir.path()).await;
        let key_id = vault_key(&state).await;

        // The row as it comes back from disk: `has_password` is false because the
        // host only fills it in when it lists, so the secret must be found by id.
        let mut conn = usable("c1", "127.0.0.1");
        conn.port = 1;
        assert!(!conn.has_password);
        state.store.upsert_connection(conn.clone()).unwrap();
        state
            .vault
            .lock()
            .await
            .as_mut()
            .unwrap()
            .put(
                &conn.id,
                Secret::Password {
                    password: "server-pw".into(),
                },
            )
            .unwrap();

        match connections_provision(&state, "c1".into(), String::new(), vec![key_id], false).await {
            Err(Error::Unreachable(m)) => {
                assert!(m.contains("127.0.0.1:1"), "message was {m:?}");
            }
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn provisioning_without_any_password_is_invalid_input() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        unlock(&state, dir.path()).await;
        let key_id = vault_key(&state).await;

        let mut conn = usable("c1", "127.0.0.1");
        conn.port = 1;
        state.store.upsert_connection(conn).unwrap();

        match connections_provision(&state, "c1".into(), String::new(), vec![key_id], false).await {
            Err(Error::InvalidInput(m)) => {
                assert_eq!(m, "no password given and none saved for this connection")
            }
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
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
        // The askpass helper cannot run on Windows, so `ssh_invocation`
        // deliberately omits the single-prompt limit there.
        #[cfg(not(windows))]
        assert!(cmd.contains("NumberOfPasswordPrompts=1"), "{cmd}");
        #[cfg(windows)]
        assert!(!cmd.contains("NumberOfPasswordPrompts"), "{cmd}");

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
    async fn a_client_supplied_password_flag_is_not_persisted_on_update() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        state
            .store
            .upsert_connection(Connection::managed("c1", "web"))
            .unwrap();
        let mut c = Connection::managed("c1", "web");
        c.has_password = true;
        assert!(matches!(
            handle(&state, Request::ConnectionsUpdate { connection: c }).await,
            Response::Ok
        ));
        let stored = state.store.load_data().unwrap().connections;
        assert!(!stored[0].has_password);
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn a_client_supplied_active_flag_is_not_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        let mut c = Connection::managed("c1", "web");
        c.active = true;
        assert!(matches!(
            handle(&state, Request::ConnectionsAdd { connection: c }).await,
            Response::Ok
        ));
        let stored = state.store.load_data().unwrap().connections;
        assert!(!stored[0].active);
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn a_client_supplied_active_flag_is_not_persisted_on_update() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        state
            .store
            .upsert_connection(Connection::managed("c1", "web"))
            .unwrap();
        let mut c = Connection::managed("c1", "web");
        c.active = true;
        assert!(matches!(
            handle(&state, Request::ConnectionsUpdate { connection: c }).await,
            Response::Ok
        ));
        let stored = state.store.load_data().unwrap().connections;
        assert!(!stored[0].active);
        state.shutdown.cancel();
    }

    /// A credential with a key: enough to turn password auth off.
    fn keyed_credential(id: &str) -> Credential {
        Credential {
            id: id.into(),
            label: "prod".into(),
            user: "deploy".into(),
            key_id: Some("k1".into()),
            has_password: false,
        }
    }

    /// A credential with only a password: it needs password auth left on.
    fn keyless_credential(id: &str) -> Credential {
        Credential {
            key_id: None,
            has_password: true,
            ..keyed_credential(id)
        }
    }

    fn store_credential(state: &HostState, credential: Credential) {
        state
            .store
            .update_data(|data| {
                data.credentials.push(credential);
                Ok(())
            })
            .unwrap();
    }

    #[tokio::test]
    async fn a_connection_cannot_store_a_credential_alongside_its_own_identity() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        store_credential(&state, keyed_credential("cred1"));

        let mut hybrid = Connection::managed("c1", "web");
        hybrid.credential_id = Some("cred1".into());
        hybrid.user = "me".into();
        match handle(&state, Request::ConnectionsAdd { connection: hybrid }).await {
            Response::Error { code, message } => {
                assert_eq!(code, "invalid_input");
                assert!(message.contains("credential"), "message was: {message}");
            }
            other => panic!("unexpected {other:?}"),
        }
        // Refused, not quietly stripped back to a credential-only connection.
        assert!(state.store.load_data().unwrap().connections.is_empty());

        let mut shared = Connection::managed("c1", "web");
        shared.credential_id = Some("cred1".into());
        assert!(matches!(
            handle(&state, Request::ConnectionsAdd { connection: shared }).await,
            Response::Ok
        ));
        let stored = state.store.load_data().unwrap().connections;
        assert_eq!(stored[0].credential_id.as_deref(), Some("cred1"));
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn an_update_cannot_turn_a_connection_into_a_hybrid() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        store_credential(&state, keyed_credential("cred1"));
        state
            .store
            .upsert_connection(Connection::managed("c1", "web"))
            .unwrap();

        let mut hybrid = Connection::managed("c1", "web");
        hybrid.credential_id = Some("cred1".into());
        hybrid.key_id = Some("k1".into());
        assert!(matches!(
            handle(&state, Request::ConnectionsUpdate { connection: hybrid }).await,
            Response::Error { code, .. } if code == "invalid_input"
        ));
        let stored = state.store.load_data().unwrap().connections;
        assert_eq!(stored[0].credential_id, None);
        assert_eq!(stored[0].key_id, None);
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn adopting_a_credential_is_refused_while_a_saved_password_exists() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        store_credential(&state, keyed_credential("cred1"));
        unlock(&state, dir.path()).await;
        // Both the stored flag and the vault say this connection owns a password.
        let mut own = Connection::managed("c1", "web");
        own.has_password = true;
        state.store.upsert_connection(own).unwrap();
        connections_set_password(&state, "c1".into(), Some("s3cret".into()))
            .await
            .unwrap();

        let mut adopting = Connection::managed("c1", "web");
        adopting.credential_id = Some("cred1".into());
        match handle(
            &state,
            Request::ConnectionsUpdate {
                connection: adopting,
            },
        )
        .await
        {
            Response::Error { code, message } => {
                assert_eq!(code, "invalid_input");
                assert!(message.contains("saved password"), "message was: {message}");
            }
            other => panic!("unexpected {other:?}"),
        }
        // Nothing was removed to make the switch work: not the secret ...
        match password_get(&state, "c1".into()).await {
            Ok(Response::Password(password)) => assert_eq!(password, "s3cret"),
            other => panic!("unexpected {other:?}"),
        }
        // ... and not the stored row.
        let stored = state.store.load_data().unwrap().connections;
        assert_eq!(stored[0].credential_id, None);
        assert!(stored[0].has_password);
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn a_locked_vault_refuses_to_guess_whether_a_switch_orphans_a_password() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        store_credential(&state, keyed_credential("cred1"));
        unlock_at_the_host_path(&state).await;
        state
            .store
            .upsert_connection(Connection::managed("c1", "web"))
            .unwrap();
        connections_set_password(&state, "c1".into(), Some("s3cret".into()))
            .await
            .unwrap();
        assert!(matches!(
            handle(&state, Request::VaultLock).await,
            Response::Ok
        ));

        let adopting = || {
            let mut c = Connection::managed("c1", "web");
            c.credential_id = Some("cred1".into());
            c
        };
        // Locked, the host cannot see the secret, so it must not let the switch
        // through and leave an unreachable password behind.
        match handle(
            &state,
            Request::ConnectionsUpdate {
                connection: adopting(),
            },
        )
        .await
        {
            Response::Error { code, message } => {
                assert_eq!(code, "vault_locked");
                assert!(
                    message.contains("unlock the vault"),
                    "message was: {message}"
                );
            }
            other => panic!("unexpected {other:?}"),
        }
        let stored = state.store.load_data().unwrap().connections;
        assert_eq!(stored[0].credential_id, None);

        // Unlocked, the refusal names the real obstacle.
        assert!(matches!(
            handle(
                &state,
                Request::VaultUnlock {
                    passphrase: "pw".into()
                }
            )
            .await,
            Response::Unlocked { .. }
        ));
        match handle(
            &state,
            Request::ConnectionsUpdate {
                connection: adopting(),
            },
        )
        .await
        {
            Response::Error { code, message } => {
                assert_eq!(code, "invalid_input");
                assert!(message.contains("saved password"), "message was: {message}");
            }
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn once_unlocked_a_switch_succeeds_when_no_password_is_saved() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        store_credential(&state, keyed_credential("cred1"));
        unlock_at_the_host_path(&state).await;
        state
            .store
            .upsert_connection(Connection::managed("c1", "web"))
            .unwrap();
        assert!(matches!(
            handle(&state, Request::VaultLock).await,
            Response::Ok
        ));

        let adopting = || {
            let mut c = Connection::managed("c1", "web");
            c.credential_id = Some("cred1".into());
            c
        };
        // No secret exists, but the locked host cannot know that either.
        assert!(matches!(
            handle(
                &state,
                Request::ConnectionsUpdate {
                    connection: adopting()
                }
            )
            .await,
            Response::Error { code, .. } if code == "vault_locked"
        ));
        assert!(matches!(
            handle(
                &state,
                Request::VaultUnlock {
                    passphrase: "pw".into()
                }
            )
            .await,
            Response::Unlocked { .. }
        ));
        assert!(matches!(
            handle(
                &state,
                Request::ConnectionsUpdate {
                    connection: adopting()
                }
            )
            .await,
            Response::Ok
        ));
        let stored = state.store.load_data().unwrap().connections;
        assert_eq!(stored[0].credential_id.as_deref(), Some("cred1"));
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn a_keyless_credential_may_not_disable_password_auth() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        store_credential(&state, keyless_credential("cred1"));

        let conn = |disable: bool| {
            let mut c = Connection::managed("c1", "web");
            c.credential_id = Some("cred1".into());
            c.disable_password_auth = disable;
            c
        };
        match handle(
            &state,
            Request::ConnectionsAdd {
                connection: conn(true),
            },
        )
        .await
        {
            Response::Error { code, message } => {
                assert_eq!(code, "invalid_input");
                assert!(
                    message.contains("password authentication"),
                    "message was: {message}"
                );
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(state.store.load_data().unwrap().connections.is_empty());

        // With password auth left on the same connection is storable.
        assert!(matches!(
            handle(
                &state,
                Request::ConnectionsAdd {
                    connection: conn(false)
                }
            )
            .await,
            Response::Ok
        ));
        // A credential that carries a key can disable it.
        state
            .store
            .update_data(|data| {
                data.credentials[0].key_id = Some("k1".into());
                Ok(())
            })
            .unwrap();
        assert!(matches!(
            handle(
                &state,
                Request::ConnectionsUpdate {
                    connection: conn(true)
                }
            )
            .await,
            Response::Ok
        ));
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn setting_and_clearing_a_credential_password() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        unlock(&state, dir.path()).await;
        let mut cred = keyed_credential("cred1");
        cred.has_password = false;
        store_credential(&state, cred);

        let listed = async || match handle(&state, Request::CredentialsList).await {
            Response::Credentials(cs) => cs[0].has_password,
            other => panic!("unexpected {other:?}"),
        };
        assert!(!listed().await);

        assert!(matches!(
            credentials_set_password(&state, "cred1".into(), Some("s3cret".into())).await,
            Ok(Response::Ok)
        ));
        assert!(listed().await);
        // The secret lives only in the vault, never in the metadata file.
        let data_toml = std::fs::read_to_string(state.paths.data_file()).unwrap();
        assert!(!data_toml.contains("s3cret"), "{data_toml}");

        // An empty password is refused, like a connection's.
        match credentials_set_password(&state, "cred1".into(), Some(String::new())).await {
            Err(Error::InvalidInput(m)) => assert_eq!(m, "password must not be empty"),
            other => panic!("unexpected {other:?}"),
        }
        assert!(listed().await);

        assert!(matches!(
            credentials_set_password(&state, "cred1".into(), None).await,
            Ok(Response::Ok)
        ));
        assert!(!listed().await);
        // The clear reaches the vault, not only the flag: a surviving secret
        // would still be served to a later prompt.
        let guard = state.vault.lock().await;
        assert!(guard.as_ref().unwrap().get("cred1").is_none());
        drop(guard);
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn clearing_the_password_of_a_keyless_credential_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        unlock(&state, dir.path()).await;
        store_credential(&state, keyless_credential("cred1"));
        credentials_set_password(&state, "cred1".into(), Some("s3cret".into()))
            .await
            .unwrap();

        match credentials_set_password(&state, "cred1".into(), None).await {
            Err(Error::InvalidInput(m)) => {
                assert!(m.contains("prod"), "message was: {m}");
                assert!(m.contains("delete"), "message was: {m}");
            }
            other => panic!("unexpected {other:?}"),
        }
        // Refused, not emptied: the row still claims a password and the vault
        // still holds it, so no connection resolves to a keyless, secretless
        // identity.
        let stored = &state.store.load_data().unwrap().credentials[0];
        assert!(stored.has_password);
        let guard = state.vault.lock().await;
        match guard.as_ref().unwrap().get("cred1") {
            Some(Secret::Password { password }) => assert_eq!(password, "s3cret"),
            _ => panic!("the refused clear must not touch the vault"),
        }
        drop(guard);
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn setting_a_credential_password_needs_an_unlocked_vault() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        store_credential(&state, keyed_credential("cred1"));
        match credentials_set_password(&state, "cred1".into(), Some("s3cret".into())).await {
            Err(Error::VaultLocked) => {}
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn setting_a_password_for_an_unknown_credential_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        unlock(&state, dir.path()).await;
        match credentials_set_password(&state, "absent".into(), Some("s3cret".into())).await {
            Err(Error::NotFound(id)) => assert_eq!(id, "absent"),
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }

    /// A connection that shares `credential_id` and owns nothing else.
    fn sharing(id: &str, credential_id: &str) -> Connection {
        let mut conn = Connection::managed(id, id);
        conn.credential_id = Some(credential_id.into());
        conn
    }

    #[tokio::test]
    async fn removing_a_credential_used_by_two_connections_names_the_count() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        unlock(&state, dir.path()).await;
        store_credential(&state, keyed_credential("cred1"));
        credentials_set_password(&state, "cred1".into(), Some("s3cret".into()))
            .await
            .unwrap();
        for id in ["c1", "c2"] {
            state.store.upsert_connection(sharing(id, "cred1")).unwrap();
        }

        match credentials_remove(&state, "cred1".into()).await {
            Err(Error::InvalidInput(m)) => assert!(m.contains('2'), "message was: {m}"),
            other => panic!("unexpected {other:?}"),
        }
        // Refused, not half-deleted: the row and its secret both survive.
        assert_eq!(state.store.load_data().unwrap().credentials.len(), 1);
        let guard = state.vault.lock().await;
        assert!(guard.as_ref().unwrap().get("cred1").is_some());
        drop(guard);
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn removing_an_unused_credential_takes_its_vault_entry() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        unlock(&state, dir.path()).await;
        store_credential(&state, keyed_credential("cred1"));
        credentials_set_password(&state, "cred1".into(), Some("s3cret".into()))
            .await
            .unwrap();

        assert!(matches!(
            credentials_remove(&state, "cred1".into()).await,
            Ok(Response::Ok)
        ));
        assert!(state.store.load_data().unwrap().credentials.is_empty());
        let guard = state.vault.lock().await;
        assert!(
            guard.as_ref().unwrap().get("cred1").is_none(),
            "the secret must not outlive its credential"
        );
        drop(guard);
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn removing_an_unknown_credential_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        match credentials_remove(&state, "absent".into()).await {
            Err(Error::NotFound(m)) => assert!(m.contains("absent"), "message was: {m}"),
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }

    fn new_credential(label: &str, user: &str) -> Credential {
        Credential {
            id: String::new(),
            label: label.into(),
            user: user.into(),
            key_id: Some("k1".into()),
            has_password: false,
        }
    }

    #[tokio::test]
    async fn adding_a_credential_assigns_an_id_and_stores_metadata_only() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        assert!(matches!(
            credentials_add(&state, new_credential("prod", "deploy"), None).await,
            Ok(Response::Ok)
        ));
        let stored = state.store.load_data().unwrap().credentials;
        assert_eq!(stored.len(), 1);
        assert!(!stored[0].id.is_empty());
        assert_eq!(stored[0].label, "prod");
        assert_eq!(stored[0].user, "deploy");
        assert!(!stored[0].has_password, "no password was supplied");

        // The vault keyspace is shared with connection passwords, so a caller
        // cannot choose an id: a connection's id here would overwrite its secret.
        let mut chosen = new_credential("other", "deploy");
        chosen.id = "c1".into();
        match credentials_add(&state, chosen, None).await {
            Err(Error::InvalidInput(m)) => {
                assert!(m.contains("assigned by the host"), "message was: {m}")
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(state.store.load_data().unwrap().credentials.len(), 1);
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn adding_with_a_password_seals_it_with_the_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        unlock(&state, dir.path()).await;
        // A password-only credential: valid only because the supplied password
        // decides the flag, whatever the payload says.
        let mut credential = new_credential("prod", "deploy");
        credential.key_id = None;
        credential.has_password = false;
        assert!(matches!(
            credentials_add(&state, credential, Some("s3cret".into())).await,
            Ok(Response::Ok)
        ));

        let stored = state.store.load_data().unwrap().credentials;
        assert!(stored[0].has_password);
        let guard = state.vault.lock().await;
        match guard.as_ref().unwrap().get(&stored[0].id) {
            Some(Secret::Password { password }) => assert_eq!(password, "s3cret"),
            _ => panic!("the vault should hold the password"),
        }
        drop(guard);
        // The secret lives only in the vault, never in the metadata file.
        let data_toml = std::fs::read_to_string(state.paths.data_file()).unwrap();
        assert!(!data_toml.contains("s3cret"), "{data_toml}");
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn adding_with_a_password_needs_an_unlocked_vault() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        let mut credential = new_credential("prod", "deploy");
        credential.key_id = None;
        match credentials_add(&state, credential, Some("s3cret".into())).await {
            Err(Error::VaultLocked) => {}
            other => panic!("unexpected {other:?}"),
        }
        // Refused before any write: no row claiming a secret that is not there.
        assert!(state.store.load_data().unwrap().credentials.is_empty());
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn adding_with_an_empty_password_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        unlock(&state, dir.path()).await;
        let mut credential = new_credential("prod", "deploy");
        credential.key_id = None;
        match credentials_add(&state, credential, Some(String::new())).await {
            Err(Error::InvalidInput(m)) => assert_eq!(m, "password must not be empty"),
            other => panic!("unexpected {other:?}"),
        }
        assert!(state.store.load_data().unwrap().credentials.is_empty());
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn the_save_path_refuses_a_duplicate_credential_label() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        assert!(matches!(
            credentials_add(&state, new_credential("prod", "deploy"), None).await,
            Ok(Response::Ok)
        ));
        // Second `prod`: the CLI's `--credential prod` would have two answers.
        match credentials_add(&state, new_credential("prod", "other"), None).await {
            Err(Error::InvalidInput(m)) => assert!(m.contains("prod"), "message was: {m}"),
            other => panic!("unexpected {other:?}"),
        }
        // Case only differs, and resolution lowercases, so it is the same label.
        match credentials_add(&state, new_credential("PROD", "other"), None).await {
            Err(Error::InvalidInput(m)) => assert!(m.contains("PROD"), "message was: {m}"),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(state.store.load_data().unwrap().credentials.len(), 1);

        // An edit onto an existing label is refused too, not only an add.
        assert!(matches!(
            credentials_add(&state, new_credential("staging", "deploy"), None).await,
            Ok(Response::Ok)
        ));
        let staging = state.store.load_data().unwrap().credentials[1].clone();
        let mut renamed = staging.clone();
        renamed.label = "prod".into();
        match credentials_update(&state, renamed).await {
            Err(Error::InvalidInput(m)) => assert!(m.contains("prod"), "message was: {m}"),
            other => panic!("unexpected {other:?}"),
        }
        // Saving a row unchanged must not collide with itself.
        assert!(matches!(
            credentials_update(&state, staging).await,
            Ok(Response::Ok)
        ));
        let stored = state.store.load_data().unwrap().credentials;
        assert_eq!(stored[1].label, "staging");
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn updating_a_credential_keeps_its_stored_password_flag() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        unlock(&state, dir.path()).await;
        let mut stored = new_credential("prod", "deploy");
        stored.key_id = None;
        stored.has_password = true;
        store_credential(&state, stored.clone());
        credentials_set_password(&state, stored.id.clone(), Some("s3cret".into()))
            .await
            .unwrap();

        // The caller sends no `has_password`; the row must not lose the secret it has.
        let mut edited = stored.clone();
        edited.user = "deployer".into();
        edited.has_password = false;
        assert!(matches!(
            credentials_update(&state, edited).await,
            Ok(Response::Ok)
        ));
        let stored = state.store.load_data().unwrap().credentials;
        assert_eq!(stored[0].user, "deployer");
        assert!(stored[0].has_password, "the vault still holds a password");
        // Not only the flag: an update that deleted the secret would fail here.
        let guard = state.vault.lock().await;
        match guard.as_ref().unwrap().get(&stored[0].id) {
            Some(Secret::Password { password }) => assert_eq!(password, "s3cret"),
            _ => panic!("the update must not touch the password"),
        }
        drop(guard);
        state.shutdown.cancel();
    }

    #[test]
    fn the_askpass_prefix_follows_the_resolved_password() {
        let mut conn = Connection::managed("c1", "web");
        conn.credential_id = Some("cred1".into());
        let socket = Path::new("/tmp/sigrun.sock");

        // A credential carrying a password hands ssh the helper.
        let auth = effective(&conn, &[keyless_credential("cred1")]).unwrap();
        let prefix = askpass_prefix(&auth, socket, "c1").expect("a stored password needs askpass");
        assert!(prefix.contains("SIGRUN_CONNECTION_ID=c1"), "{prefix}");
        assert!(prefix.contains("sigrun.sock"), "{prefix}");

        // A key-only credential must not: ssh would then have no one to ask.
        let auth = effective(&conn, &[keyed_credential("cred1")]).unwrap();
        assert!(askpass_prefix(&auth, socket, "c1").is_none());
    }

    #[tokio::test]
    async fn connecting_through_a_credential_uses_its_identity_and_password() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        unlock(&state, dir.path()).await;
        store_credential(&state, keyless_credential("cred1"));
        credentials_set_password(&state, "cred1".into(), Some("s3cret".into()))
            .await
            .unwrap();
        let mut conn = Connection::managed("c1", "web");
        conn.host = "example.com".into();
        conn.credential_id = Some("cred1".into());
        state.store.upsert_connection(conn).unwrap();

        let Response::Command(cmd) = connections_connect(&state, "c1".into(), true, false)
            .await
            .unwrap()
        else {
            panic!("expected a command");
        };
        // The user comes from the credential, not the connection's empty field.
        assert!(cmd.contains("deploy@example.com"), "{cmd}");
        #[cfg(not(windows))]
        assert!(cmd.contains("NumberOfPasswordPrompts=1"), "{cmd}");

        // The askpass helper asks by connection id; the host resolves the
        // credential, so rotating it is what every connection reads.
        match password_get(&state, "c1".into()).await {
            Ok(Response::Password(password)) => assert_eq!(password, "s3cret"),
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn connecting_through_a_missing_credential_names_it() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        let mut conn = Connection::managed("c1", "web");
        conn.host = "example.com".into();
        conn.credential_id = Some("gone".into());
        state.store.upsert_connection(conn).unwrap();

        // No fallback to the connection's own (empty) user, and no prompt.
        match connections_connect(&state, "c1".into(), true, false).await {
            Err(Error::InvalidInput(m)) => {
                assert!(m.contains("gone"), "message was: {m}");
            }
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn listing_connections_reports_the_host_active_set_not_the_client() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        for id in ["c1", "c2"] {
            state
                .store
                .upsert_connection(Connection::managed(id, "web"))
                .unwrap();
        }
        // A stale `active` already in the store must not survive the response.
        let mut stale = Connection::managed("c2", "web");
        stale.active = true;
        state.store.upsert_connection(stale).unwrap();

        state.mark_active("c1");
        match handle(&state, Request::ConnectionsList).await {
            Response::Connections(cs) => {
                assert!(cs.iter().find(|c| c.id == "c1").unwrap().active);
                assert!(!cs.iter().find(|c| c.id == "c2").unwrap().active);
            }
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn connection_ended_clears_an_active_connection() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        state.mark_active("c1");
        assert!(state.is_active("c1"));
        assert!(matches!(
            handle(&state, Request::ConnectionEnded { id: "c1".into() }).await,
            Response::Ok
        ));
        assert!(!state.is_active("c1"));
        state.shutdown.cancel();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_session_report_keeps_the_connection_active_while_its_pid_lives() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        assert!(matches!(
            handle(
                &state,
                Request::SessionStarted {
                    id: "c1".into(),
                    pid: child.id()
                }
            )
            .await,
            Response::Ok
        ));
        state.mark_active("c1");
        assert!(!state.sweep_dead_sessions());
        assert!(state.is_active("c1"));
        child.kill().unwrap();
        child.wait().unwrap();
        state.shutdown.cancel();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_dead_pid_clears_the_connection_on_the_next_sweep() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        state.mark_active("c1");
        handle(
            &state,
            Request::SessionStarted {
                id: "c1".into(),
                pid,
            },
        )
        .await;
        // Kill and reap: an unreaped child is a zombie, and `kill(pid, 0)` still
        // answers "alive" for a zombie.
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(state.sweep_dead_sessions());
        assert!(!state.is_active("c1"));
        // The dead pid is forgotten too, so a later sweep has nothing to report.
        assert!(!state.sweep_dead_sessions());
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn sweeping_with_no_sessions_reports_no_change() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        // A connection nobody reported a pid for (Windows, or an older client)
        // is never swept: no report means nothing to check.
        state.mark_active("c1");
        assert!(!state.sweep_dead_sessions());
        assert!(state.is_active("c1"));
        state.shutdown.cancel();
    }

    #[cfg(unix)]
    #[test]
    fn a_watched_shell_knows_whether_it_still_has_a_parent() {
        let me = std::process::id();
        // SAFETY: getppid takes no arguments and cannot fail.
        let parent = unsafe { libc::getppid() } as u32;
        assert_eq!(parent_of(me), Some(parent));
        // The test runner's parent is the harness, not init: it is not an
        // orphan, so a live session must survive the sweep.
        assert!(!session_is_orphaned(me));
    }

    #[tokio::test]
    async fn a_session_report_is_not_a_vault_touching_request() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        let before = state
            .last_vault_activity
            .load(std::sync::atomic::Ordering::Relaxed);
        assert!(matches!(
            handle(
                &state,
                Request::SessionStarted {
                    id: "c1".into(),
                    pid: std::process::id()
                }
            )
            .await,
            Response::Ok
        ));
        assert_eq!(
            state
                .last_vault_activity
                .load(std::sync::atomic::Ordering::Relaxed),
            before
        );
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn connection_ended_for_an_unknown_id_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        state.mark_active("c1");
        assert!(matches!(
            handle(&state, Request::ConnectionEnded { id: "nope".into() }).await,
            Response::Ok
        ));
        assert_eq!(state.active_ids(), vec!["c1".to_string()]);
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn locking_the_vault_clears_the_active_set() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        state.mark_active("c2");
        state.mark_active("c1");
        assert_eq!(state.active_ids(), vec!["c1".to_string(), "c2".to_string()]);
        assert!(matches!(
            handle(&state, Request::VaultLock).await,
            Response::Ok
        ));
        assert!(state.active_ids().is_empty());
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
    async fn removing_a_connection_clears_its_active_id() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        state
            .store
            .upsert_connection(Connection::managed("c1", "web"))
            .unwrap();
        state.mark_active("c1");
        state.report_session("c1", 4242);
        assert_eq!(state.active_ids(), vec!["c1".to_string()]);
        assert!(matches!(
            handle(&state, Request::ConnectionsRemove { id: "c1".into() }).await,
            Response::Ok
        ));
        assert!(state.active_ids().is_empty());
        // The watched pid goes with it, so nothing is left to sweep.
        assert!(state.session_map().is_empty());
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn a_second_session_report_replaces_the_watched_pid() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        state.report_session("c1", 1111);
        state.report_session("c1", 2222);
        assert_eq!(state.session_map().get("c1"), Some(&2222));
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn locking_the_vault_forgets_the_watched_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        state.report_session("c1", 1111);
        state.mark_active("c1");
        state.lock_vault().await;
        assert!(state.session_map().is_empty());
        assert!(state.active_ids().is_empty());
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

    #[tokio::test]
    async fn exporting_writes_a_folder_the_import_can_read_back() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        let source = host_with_one_connection(from.path()).await;
        let target = to.path().join("sigrun-export");

        let Response::Export(summary) = handle(
            &source,
            Request::ExportBundle {
                path: target.display().to_string(),
                zipped: false,
            },
        )
        .await
        else {
            panic!("the export should have succeeded");
        };
        assert_eq!(summary.connections, 1);
        assert_eq!(summary.keys, 0);
        assert_eq!(summary.path, target.display().to_string());

        // A second host whose data dir has nothing in it.
        let dest = state_in(to.path()).await;
        let Response::Import(summary) = handle(
            &dest,
            Request::ImportBundle {
                path: target.display().to_string(),
            },
        )
        .await
        else {
            panic!("the import should have succeeded");
        };
        assert_eq!(summary.added, 1);
        match handle(&dest, Request::ConnectionsList).await {
            Response::Connections(cs) => {
                assert_eq!(cs.len(), 1);
                assert_eq!(cs[0].id, "c1");
                assert_eq!(cs[0].host, "one.example");
            }
            other => panic!("unexpected {other:?}"),
        }
        source.shutdown.cancel();
        dest.shutdown.cancel();
    }

    #[tokio::test]
    async fn importing_twice_adds_nothing_the_second_time() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        let source = host_with_one_connection(from.path()).await;
        let target = to.path().join("sigrun-export");
        handle(
            &source,
            Request::ExportBundle {
                path: target.display().to_string(),
                zipped: false,
            },
        )
        .await;

        let dest = state_in(to.path()).await;
        let import = || Request::ImportBundle {
            path: target.display().to_string(),
        };
        match handle(&dest, import()).await {
            Response::Import(s) => {
                assert_eq!(s.added, 1);
                assert_eq!(s.skipped, 0);
            }
            other => panic!("unexpected {other:?}"),
        }
        match handle(&dest, import()).await {
            Response::Import(s) => {
                assert_eq!(s.added, 0);
                assert_eq!(s.skipped, 1);
            }
            other => panic!("unexpected {other:?}"),
        }
        // Skipped means not stored again either: one connection, not two.
        assert_eq!(dest.store.load_data().unwrap().connections.len(), 1);
        source.shutdown.cancel();
        dest.shutdown.cancel();
    }

    #[tokio::test]
    async fn importing_sets_no_password_and_no_active_flag() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        // `build_bundle` clears these, so a file that sets them is the only way
        // to pin that import clears them again.
        let mut conn = usable("c1", "one.example");
        conn.has_password = true;
        conn.active = true;
        let hostile = bundle::Bundle {
            format: bundle::FORMAT_VERSION,
            exported_at: 0,
            connections: vec![conn],
            keys: Vec::new(),
        };
        let folder = dir.path().join("export");
        bundle::write_folder(&hostile, &folder).unwrap();

        assert!(matches!(
            handle(
                &state,
                Request::ImportBundle {
                    path: folder.display().to_string()
                }
            )
            .await,
            Response::Import(_)
        ));
        let stored = state.store.load_data().unwrap().connections;
        assert_eq!(stored.len(), 1);
        assert!(!stored[0].has_password && !stored[0].active);
        // The list is what the UI reads, so it must report the same.
        match handle(&state, Request::ConnectionsList).await {
            Response::Connections(cs) => assert!(!cs[0].has_password && !cs[0].active),
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn importing_does_not_copy_an_invalid_connection_into_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        // A crafted export whose host the launcher would refuse. Storing the
        // parsed file verbatim would keep it; import merges instead.
        let mut conn = usable("c1", "placeholder");
        conn.host = String::new();
        let hostile = bundle::Bundle {
            format: bundle::FORMAT_VERSION,
            exported_at: 0,
            connections: vec![conn],
            keys: Vec::new(),
        };
        let folder = dir.path().join("export");
        bundle::write_folder(&hostile, &folder).unwrap();

        match handle(
            &state,
            Request::ImportBundle {
                path: folder.display().to_string(),
            },
        )
        .await
        {
            Response::Import(s) => {
                assert_eq!(s.added, 0);
                assert_eq!(s.invalid, 1);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(state.store.load_data().unwrap().connections.is_empty());
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn exporting_into_the_ssh_directory_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let state = host_with_one_connection(dir.path()).await;
        std::fs::create_dir_all(&state.paths.ssh_dir).unwrap();
        let target = state.paths.ssh_dir.join("sigrun-export");
        match handle(
            &state,
            Request::ExportBundle {
                path: target.display().to_string(),
                zipped: false,
            },
        )
        .await
        {
            Response::Error { code, .. } => assert_eq!(code, "invalid_input"),
            other => panic!("unexpected {other:?}"),
        }
        assert!(!target.exists(), "nothing may be written into ~/.ssh");
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn exporting_over_an_existing_export_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let state = host_with_one_connection(dir.path()).await;
        let target = dir.path().join("sigrun-export");
        let request = || Request::ExportBundle {
            path: target.display().to_string(),
            zipped: false,
        };
        assert!(matches!(
            handle(&state, request()).await,
            Response::Export(_)
        ));

        // A marker the second export would rub out if it were allowed to run.
        let export_file = target.join(bundle::EXPORT_FILE);
        std::fs::write(&export_file, b"SENTINEL").unwrap();
        match handle(&state, request()).await {
            Response::Error { code, .. } => assert_eq!(code, "already_exists"),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(std::fs::read(&export_file).unwrap(), b"SENTINEL");

        // The README is a root entry too: with `export.json` gone, its presence
        // alone must still refuse the write.
        std::fs::remove_file(&export_file).unwrap();
        match handle(&state, request()).await {
            Response::Error { code, .. } => assert_eq!(code, "already_exists"),
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn exporting_over_an_existing_zip_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let state = host_with_one_connection(dir.path()).await;
        let target = dir.path().join("backup.zip");
        std::fs::write(&target, b"SENTINEL").unwrap();
        match handle(
            &state,
            Request::ExportBundle {
                path: target.display().to_string(),
                zipped: true,
            },
        )
        .await
        {
            Response::Error { code, .. } => assert_eq!(code, "already_exists"),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(std::fs::read(&target).unwrap(), b"SENTINEL");
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn export_and_import_do_not_defer_the_idle_auto_lock() {
        let dir = tempfile::tempdir().unwrap();
        let state = host_with_one_connection(dir.path()).await;
        let target = dir.path().join("sigrun-export");
        assert!(matches!(
            handle(
                &state,
                Request::ExportBundle {
                    path: target.display().to_string(),
                    zipped: false
                }
            )
            .await,
            Response::Export(_)
        ));
        assert!(matches!(
            handle(
                &state,
                Request::ImportBundle {
                    path: target.display().to_string()
                }
            )
            .await,
            Response::Import(_)
        ));
        // Neither reads the vault, so neither may bump the idle clock.
        assert_eq!(
            state
                .last_vault_activity
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn a_missing_bundle_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path()).await;
        match handle(
            &state,
            Request::ImportBundle {
                path: dir.path().join("nope").display().to_string(),
            },
        )
        .await
        {
            Response::Error { code, .. } => assert_eq!(code, "not_found"),
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }
}
