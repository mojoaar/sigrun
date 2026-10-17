use std::sync::Arc;

use serde::Serialize;
use sigrun_core::host::{handle, HostState};
use sigrun_core::ipc::protocol::{Request, Response};
use sigrun_core::model::{Connection, KeyAlgorithm, Settings};
use tauri::State;

#[derive(Debug, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CommandError {}

type CmdResult = Result<Response, CommandError>;

async fn call(state: Arc<HostState>, req: Request) -> CmdResult {
    match handle(&state, req).await {
        Response::Error { code, message } => Err(CommandError { code, message }),
        other => Ok(other),
    }
}

fn config_err(e: impl std::fmt::Display) -> CommandError {
    CommandError {
        code: "invalid_config".into(),
        message: e.to_string(),
    }
}

pub async fn get_status_inner(state: Arc<HostState>) -> CmdResult {
    call(state, Request::Status).await
}
pub async fn vault_init_inner(state: Arc<HostState>, passphrase: String) -> CmdResult {
    call(state, Request::VaultInit { passphrase }).await
}
pub async fn vault_unlock_inner(state: Arc<HostState>, passphrase: String) -> CmdResult {
    call(state, Request::VaultUnlock { passphrase }).await
}
pub async fn vault_lock_inner(state: Arc<HostState>) -> CmdResult {
    call(state, Request::VaultLock).await
}
pub async fn list_connections_inner(state: Arc<HostState>) -> CmdResult {
    call(state, Request::ConnectionsList).await
}
pub async fn add_connection_inner(state: Arc<HostState>, conn: Connection) -> CmdResult {
    call(state, Request::ConnectionsAdd { connection: conn }).await
}
pub async fn update_connection_inner(state: Arc<HostState>, conn: Connection) -> CmdResult {
    call(state, Request::ConnectionsUpdate { connection: conn }).await
}
pub async fn remove_connection_inner(state: Arc<HostState>, id: String) -> CmdResult {
    call(state, Request::ConnectionsRemove { id }).await
}
pub async fn connect_inner(state: Arc<HostState>, id: String, print_only: bool) -> CmdResult {
    call(state, Request::ConnectionsConnect { id, print_only }).await
}
pub async fn list_keys_inner(state: Arc<HostState>) -> CmdResult {
    call(state, Request::KeysList).await
}
pub async fn generate_key_inner(
    state: Arc<HostState>,
    name: String,
    algorithm: KeyAlgorithm,
    passphrase: Option<String>,
) -> CmdResult {
    call(
        state,
        Request::KeysGenerate {
            name,
            algorithm,
            passphrase,
        },
    )
    .await
}
pub async fn import_key_inner(
    state: Arc<HostState>,
    path: String,
    name: String,
    passphrase: Option<String>,
) -> CmdResult {
    call(
        state,
        Request::KeysImport {
            path: path.into(),
            name,
            passphrase,
        },
    )
    .await
}
pub async fn remove_key_inner(state: Arc<HostState>, id: String) -> CmdResult {
    call(state, Request::KeysRemove { id }).await
}
pub async fn get_settings_inner(state: Arc<HostState>) -> CmdResult {
    let s = state.store.load_settings().map_err(config_err)?;
    Ok(Response::Command(
        serde_json::to_string(&s).map_err(config_err)?,
    ))
}
pub async fn save_settings_inner(state: Arc<HostState>, settings: Settings) -> CmdResult {
    state.store.save_settings(&settings).map_err(config_err)?;
    Ok(Response::Ok)
}

type St<'a> = State<'a, Arc<HostState>>;

#[tauri::command]
pub async fn get_status(state: St<'_>) -> CmdResult {
    get_status_inner(state.inner().clone()).await
}
#[tauri::command]
pub async fn vault_init(state: St<'_>, passphrase: String) -> CmdResult {
    vault_init_inner(state.inner().clone(), passphrase).await
}
#[tauri::command]
pub async fn vault_unlock(state: St<'_>, passphrase: String) -> CmdResult {
    vault_unlock_inner(state.inner().clone(), passphrase).await
}
#[tauri::command]
pub async fn vault_lock(state: St<'_>) -> CmdResult {
    vault_lock_inner(state.inner().clone()).await
}
#[tauri::command]
pub async fn list_connections(state: St<'_>) -> CmdResult {
    list_connections_inner(state.inner().clone()).await
}
#[tauri::command]
pub async fn add_connection(state: St<'_>, conn: Connection) -> CmdResult {
    add_connection_inner(state.inner().clone(), conn).await
}
#[tauri::command]
pub async fn update_connection(state: St<'_>, conn: Connection) -> CmdResult {
    update_connection_inner(state.inner().clone(), conn).await
}
#[tauri::command]
pub async fn remove_connection(state: St<'_>, id: String) -> CmdResult {
    remove_connection_inner(state.inner().clone(), id).await
}
#[tauri::command]
pub async fn connect(state: St<'_>, id: String, print_only: bool) -> CmdResult {
    connect_inner(state.inner().clone(), id, print_only).await
}
#[tauri::command]
pub async fn list_keys(state: St<'_>) -> CmdResult {
    list_keys_inner(state.inner().clone()).await
}
#[tauri::command]
pub async fn generate_key(
    state: St<'_>,
    name: String,
    algorithm: KeyAlgorithm,
    passphrase: Option<String>,
) -> CmdResult {
    generate_key_inner(state.inner().clone(), name, algorithm, passphrase).await
}
#[tauri::command]
pub async fn import_key(
    state: St<'_>,
    path: String,
    name: String,
    passphrase: Option<String>,
) -> CmdResult {
    import_key_inner(state.inner().clone(), path, name, passphrase).await
}
#[tauri::command]
pub async fn remove_key(state: St<'_>, id: String) -> CmdResult {
    remove_key_inner(state.inner().clone(), id).await
}
#[tauri::command]
pub async fn get_settings(state: St<'_>) -> CmdResult {
    get_settings_inner(state.inner().clone()).await
}
#[tauri::command]
pub async fn save_settings(state: St<'_>, settings: Settings) -> CmdResult {
    save_settings_inner(state.inner().clone(), settings).await
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn vault_unlock_roundtrip_through_commands() {
        let dir = tempfile::tempdir().unwrap();
        let paths = sigrun_core::paths::Paths::with_roots(
            dir.path().into(),
            dir.path().into(),
            dir.path().join("ssh"),
        );
        let state = crate::host::start_host(paths).await.unwrap();
        crate::commands::vault_init_inner(state.clone(), "pass".into())
            .await
            .unwrap();
        let out = crate::commands::vault_unlock_inner(state.clone(), "pass".into())
            .await
            .unwrap();
        assert!(matches!(
            out,
            sigrun_core::ipc::protocol::Response::Unlocked { .. }
        ));
        state.shutdown.cancel();
    }
}
