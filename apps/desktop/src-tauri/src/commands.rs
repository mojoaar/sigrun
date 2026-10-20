use std::sync::Arc;

use serde::Serialize;
use sigrun_core::host::{handle, HostState};
use sigrun_core::ipc::protocol::{Request, Response};
use sigrun_core::model::{Connection, KeyAlgorithm, Settings};
use sigrun_core::trust::{self, TrustStatus};
#[cfg(target_os = "macos")]
use tauri::Manager;
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
pub async fn set_connection_password_inner(
    state: Arc<HostState>,
    id: String,
    password: Option<String>,
) -> CmdResult {
    call(state, Request::ConnectionsSetPassword { id, password }).await
}
pub async fn connect_inner(
    state: Arc<HostState>,
    id: String,
    print_only: bool,
    force: bool,
) -> CmdResult {
    call(
        state,
        Request::ConnectionsConnect {
            id,
            print_only,
            force,
        },
    )
    .await
}
pub async fn provision_connection_inner(
    state: Arc<HostState>,
    id: String,
    password: String,
    key_ids: Vec<String>,
    disable_password_auth: bool,
) -> CmdResult {
    call(
        state,
        Request::ConnectionsProvision {
            id,
            password,
            key_ids,
            disable_password_auth,
        },
    )
    .await
}
pub async fn list_keys_inner(state: Arc<HostState>) -> CmdResult {
    call(state, Request::KeysList).await
}
pub async fn list_terminals_inner(state: Arc<HostState>) -> CmdResult {
    call(state, Request::TerminalsList).await
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
pub async fn export_bundle_inner(state: Arc<HostState>, path: String, zipped: bool) -> CmdResult {
    call(state, Request::ExportBundle { path, zipped }).await
}
pub async fn import_bundle_inner(state: Arc<HostState>, path: String) -> CmdResult {
    call(state, Request::ImportBundle { path }).await
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
pub async fn vault_init(state: St<'_>, app: tauri::AppHandle, passphrase: String) -> CmdResult {
    let st = state.inner().clone();
    let out = vault_init_inner(st.clone(), passphrase).await;
    crate::tray::sync_tray(&app, &st);
    out
}
#[tauri::command]
pub async fn vault_unlock(state: St<'_>, app: tauri::AppHandle, passphrase: String) -> CmdResult {
    let st = state.inner().clone();
    let out = vault_unlock_inner(st.clone(), passphrase).await;
    crate::tray::sync_tray(&app, &st);
    out
}
#[tauri::command]
pub async fn vault_lock(state: St<'_>, app: tauri::AppHandle) -> CmdResult {
    let st = state.inner().clone();
    let out = vault_lock_inner(st.clone()).await;
    crate::tray::sync_tray(&app, &st);
    out
}
#[tauri::command]
pub async fn list_connections(state: St<'_>) -> CmdResult {
    list_connections_inner(state.inner().clone()).await
}
#[tauri::command]
pub async fn list_terminals(state: St<'_>) -> CmdResult {
    list_terminals_inner(state.inner().clone()).await
}
#[tauri::command]
pub async fn add_connection(state: St<'_>, app: tauri::AppHandle, conn: Connection) -> CmdResult {
    let st = state.inner().clone();
    let out = add_connection_inner(st.clone(), conn).await;
    crate::tray::sync_tray(&app, &st);
    out
}
#[tauri::command]
pub async fn update_connection(
    state: St<'_>,
    app: tauri::AppHandle,
    conn: Connection,
) -> CmdResult {
    let st = state.inner().clone();
    let out = update_connection_inner(st.clone(), conn).await;
    crate::tray::sync_tray(&app, &st);
    out
}
#[tauri::command]
pub async fn remove_connection(state: St<'_>, app: tauri::AppHandle, id: String) -> CmdResult {
    let st = state.inner().clone();
    let out = remove_connection_inner(st.clone(), id).await;
    crate::tray::sync_tray(&app, &st);
    out
}
#[tauri::command]
pub async fn set_connection_password(
    state: St<'_>,
    id: String,
    password: Option<String>,
) -> CmdResult {
    set_connection_password_inner(state.inner().clone(), id, password).await
}
async fn probe_host_key(host: &str, port: u16) -> Option<Vec<(String, String)>> {
    let mut cmd = tokio::process::Command::new("ssh-keyscan");
    cmd.args([
        "-p",
        &port.to_string(),
        "-T",
        "5",
        "-t",
        "ed25519,ecdsa,rsa",
        "--",
        host,
    ])
    .stdin(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .kill_on_drop(true);
    let out = tokio::time::timeout(std::time::Duration::from_secs(5), cmd.output())
        .await
        .ok()?
        .ok()?;
    let keys = parse_keyscan(&String::from_utf8_lossy(&out.stdout));
    (!keys.is_empty()).then_some(keys)
}

fn parse_keyscan(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut p = l.split_whitespace();
            let (_, algo, key) = (p.next()?, p.next()?, p.next()?);
            Some((algo.to_string(), key.to_string()))
        })
        .collect()
}

fn key_error(code: &str, algo: &str, key: &str) -> CommandError {
    CommandError {
        code: code.into(),
        message: format!(
            "{} {algo} {key}",
            trust::fingerprint(algo, key).unwrap_or_else(|| "unknown".into())
        ),
    }
}

/// A recorded key presented by the host wins (extra unrecorded key types are not a
/// conflict); otherwise any mismatch refuses; otherwise all-unknown prompts.
fn evaluate_trust(
    kh: &std::path::Path,
    host: &str,
    port: u16,
    keys: &[(String, String)],
) -> Result<(), CommandError> {
    let mut mismatch = None;
    for (algo, key) in keys {
        match trust::check(kh, host, port, algo, key).map_err(config_err)? {
            TrustStatus::Trusted => return Ok(()),
            TrustStatus::Mismatch => mismatch = mismatch.or(Some((algo, key))),
            TrustStatus::Unknown => {}
        }
    }
    if let Some((algo, key)) = mismatch {
        return Err(key_error("host_key_mismatch", algo, key));
    }
    match keys.first() {
        Some((algo, key)) => Err(key_error("host_key_unknown", algo, key)),
        None => Ok(()),
    }
}

/// Pre-flight TOFU gate deciding whether Sigrun launches the terminal. The probe is
/// advisory: a missing `ssh-keyscan` or a failed probe skips the check (fail-open by
/// design); the `ssh` in the terminal still verifies against the user's own
/// `~/.ssh/known_hosts`.
async fn check_trust(state: &HostState, host: &str, port: u16) -> Result<(), CommandError> {
    let Some(keys) = probe_host_key(host, port).await else {
        eprintln!("sigrun: host-key probe unavailable; skipping trust check");
        return Ok(());
    };
    evaluate_trust(&state.paths.data_dir.join("known_hosts"), host, port, &keys)
}

/// Connect after the TOFU host-key check. `print_only` contacts nothing, so it skips it.
pub async fn connect_guarded(
    state: Arc<HostState>,
    id: String,
    print_only: bool,
    force: bool,
) -> CmdResult {
    if !print_only {
        let conns = match list_connections_inner(state.clone()).await? {
            Response::Connections(c) => c,
            _ => Vec::new(),
        };
        if let Some(c) = conns.iter().find(|c| c.id == id) {
            check_trust(&state, &c.host, c.port).await?;
        }
    }
    connect_inner(state, id, print_only, force).await
}

#[tauri::command]
pub async fn connect(state: St<'_>, id: String, print_only: bool, force: bool) -> CmdResult {
    connect_guarded(state.inner().clone(), id, print_only, force).await
}
#[tauri::command]
pub async fn provision_connection(
    state: St<'_>,
    id: String,
    password: String,
    key_ids: Vec<String>,
    disable_password_auth: bool,
) -> CmdResult {
    provision_connection_inner(
        state.inner().clone(),
        id,
        password,
        key_ids,
        disable_password_auth,
    )
    .await
}
#[tauri::command]
pub async fn trust_host(
    state: St<'_>,
    host: String,
    port: u16,
    algo: String,
    key_b64: String,
) -> CmdResult {
    let kh = state.paths.data_dir.join("known_hosts");
    trust::record(&kh, &host, port, &algo, &key_b64).map_err(config_err)?;
    Ok(Response::Ok)
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
pub async fn export_bundle(state: St<'_>, path: String, zipped: bool) -> CmdResult {
    export_bundle_inner(state.inner().clone(), path, zipped).await
}
#[tauri::command]
pub async fn import_bundle(state: St<'_>, path: String) -> CmdResult {
    import_bundle_inner(state.inner().clone(), path).await
}
#[tauri::command]
pub async fn get_settings(state: St<'_>) -> CmdResult {
    get_settings_inner(state.inner().clone()).await
}
#[tauri::command]
pub async fn save_settings(state: St<'_>, app: tauri::AppHandle, settings: Settings) -> CmdResult {
    let st = state.inner().clone();
    #[cfg(target_os = "macos")]
    let language = settings.language.clone();
    let out = save_settings_inner(st.clone(), settings).await;
    // The tray labels follow Settings.language.
    crate::tray::sync_tray(&app, &st);
    // The macOS menu labels follow it too. A failed save must not repaint the
    // menu with a language that was not stored.
    #[cfg(target_os = "macos")]
    if matches!(&out, Ok(Response::Ok)) {
        if let Err(e) = crate::setup_macos_menu(&app, &language) {
            eprintln!("sigrun: could not rebuild the menu: {e}");
        }
    }
    out
}

/// Repaint the main window's native appearance so the overlay title bar's
/// traffic lights match the app's Light/Dark setting. `setting` carries the raw
/// `"system" | "light" | "dark"` value; the mapping lives in `window_theme`.
/// Failures are ignored: the window just keeps its current appearance.
///
/// Only macOS draws the overlay traffic lights over our header. Elsewhere
/// `set_theme` still changes native appearance — Linux's tao maps it onto the
/// GTK prefer-dark flag, Windows repaints its (non-overlay) title bar — so the
/// call is macOS-only and the signature stays the same on every platform.
#[tauri::command]
pub fn set_window_theme(app: tauri::AppHandle, setting: String) -> CmdResult {
    #[cfg(target_os = "macos")]
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_theme(crate::window_theme(&setting));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (&app, &setting);
    Ok(Response::Ok)
}

/// External links the UI may open. The allowlist is fixed: the host is the
/// trust boundary, so the webview can never ask the OS to open anything else.
const ALLOWED_URLS: [&str; 2] = ["https://johansen.foo", "https://github.com/mojoaar/sigrun"];

fn allowed_url(url: &str) -> sigrun_core::error::Result<()> {
    ALLOWED_URLS
        .contains(&url)
        .then_some(())
        .ok_or_else(|| sigrun_core::error::Error::InvalidInput("unsupported url".into()))
}

/// Launch the OS opener detached; we never wait on the browser and discard its
/// output. `url` is already allowlisted by `allowed_url`.
fn spawn_opener(url: &str) {
    use sigrun_core::terminal::Platform;
    let (program, args): (&str, &[&str]) = match Platform::current() {
        Platform::MacOs => ("open", &[url]),
        Platform::Linux => ("xdg-open", &[url]),
        Platform::Windows => ("cmd", &["/c", "start", "", url]),
    };
    let _ = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[tauri::command]
pub async fn open_external(url: String) -> CmdResult {
    allowed_url(&url).map_err(|e| CommandError {
        code: "invalid_input".into(),
        message: e.to_string(),
    })?;
    spawn_opener(&url);
    Ok(Response::Ok)
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_all_keyscan_lines() {
        let out = "# h:22 SSH-2.0-x\nh ssh-ed25519 AAAA\nh ssh-rsa ZZZZ\n";
        assert_eq!(
            super::parse_keyscan(out),
            vec![
                ("ssh-ed25519".into(), "AAAA".into()),
                ("ssh-rsa".into(), "ZZZZ".into())
            ]
        );
        assert!(super::parse_keyscan("# only comment\n").is_empty());
    }

    #[test]
    fn multi_key_trust_precedence() {
        const KEY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIKKEEWQJy/IdduM5HglENeKuk+5Am8lcPpdlUdvxDWqq";
        let dir = tempfile::tempdir().unwrap();
        let kh = dir.path().join("known_hosts");
        sigrun_core::trust::record(&kh, "h", 22, "ssh-ed25519", KEY).unwrap();
        let k = |a: &str, b: &str| (a.to_string(), b.to_string());
        let ok = [k("ssh-rsa", "ZZZZ"), k("ssh-ed25519", KEY)];
        assert!(super::evaluate_trust(&kh, "h", 22, &ok).is_ok());
        let bad = [k("ssh-rsa", "ZZZZ"), k("ssh-ed25519", "BBBB")];
        assert_eq!(
            super::evaluate_trust(&kh, "h", 22, &bad).unwrap_err().code,
            "host_key_mismatch"
        );
        let fresh = [k("ssh-ed25519", KEY)];
        assert_eq!(
            super::evaluate_trust(&kh, "other", 22, &fresh)
                .unwrap_err()
                .code,
            "host_key_unknown"
        );
    }

    #[test]
    fn open_external_rejects_a_foreign_url() {
        assert!(super::allowed_url("https://johansen.foo").is_ok());
        assert!(super::allowed_url("https://github.com/mojoaar/sigrun").is_ok());
        let err = super::allowed_url("https://example.com/evil").unwrap_err();
        assert!(matches!(err, sigrun_core::error::Error::InvalidInput(_)));
    }

    #[tokio::test]
    async fn export_and_import_round_trip_through_commands() {
        let from = tempfile::tempdir().unwrap();
        let source = crate::host::start_host(sigrun_core::paths::Paths::with_roots(
            from.path().into(),
            from.path().into(),
            from.path().join("ssh"),
        ))
        .await
        .unwrap();
        source
            .store
            .upsert_connection(sigrun_core::model::Connection::managed("c1", "web"))
            .unwrap();
        let dest = from.path().join("sigrun-export");
        match crate::commands::export_bundle_inner(
            source.clone(),
            dest.display().to_string(),
            false,
        )
        .await
        .unwrap()
        {
            sigrun_core::ipc::protocol::Response::Export(s) => {
                assert_eq!((s.connections, s.keys), (1, 0));
                assert_eq!(s.path, dest.display().to_string());
            }
            other => panic!("unexpected {other:?}"),
        }
        source.shutdown.cancel();

        let to = tempfile::tempdir().unwrap();
        let target = crate::host::start_host(sigrun_core::paths::Paths::with_roots(
            to.path().into(),
            to.path().into(),
            to.path().join("ssh"),
        ))
        .await
        .unwrap();
        match crate::commands::import_bundle_inner(target.clone(), dest.display().to_string())
            .await
            .unwrap()
        {
            sigrun_core::ipc::protocol::Response::Import(s) => {
                assert_eq!((s.added, s.skipped, s.invalid), (1, 0, 0));
            }
            other => panic!("unexpected {other:?}"),
        }
        target.shutdown.cancel();
    }

    #[test]
    fn the_capability_pins_the_two_dialog_grants_and_nothing_wider() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let perms: Vec<&str> = v["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_str().unwrap())
            .collect();
        assert!(perms.contains(&"dialog:allow-open"));
        assert!(perms.contains(&"dialog:allow-save"));
        // The pickers are the only dialog API the webview uses, so the wider
        // `dialog:default` set stays out.
        assert!(!perms.iter().any(|p| p.starts_with("dialog:")
            && *p != "dialog:allow-open"
            && *p != "dialog:allow-save"));
    }

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
