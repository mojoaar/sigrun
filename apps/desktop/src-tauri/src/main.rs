#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // ssh runs this binary as its `SSH_ASKPASS` helper and passes the prompt as
    // the only argument. Require the explicit marker *and* a connection id (see
    // `terminal::env_prefix`) so an ordinary launch with a stray environment is
    // never hijacked. Handle both shapes before anything else, so no window,
    // tray, or host starts.
    let args: Vec<String> = std::env::args().collect();
    let marker = args.get(1).map(String::as_str) == Some("--askpass");
    let from_ssh = args.len() == 2
        && std::env::var("SIGRUN_ASKPASS").as_deref() == Ok("1")
        && std::env::var("SIGRUN_CONNECTION_ID").is_ok_and(|id| !id.is_empty());
    if marker || from_ssh {
        std::process::exit(askpass());
    }
    sigrun_desktop::run()
}

/// Print the stored password on stdout and nothing else; every failure exits 2
/// with a fixed reason, never the password or the underlying error.
fn askpass() -> i32 {
    let socket = std::env::var_os("SIGRUN_SOCKET")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            sigrun_core::paths::Paths::resolve()
                .ok()
                .map(|p| p.socket_path())
        });
    let id = std::env::var("SIGRUN_CONNECTION_ID").ok();
    if let (Some(socket), Some(id)) = (socket, id) {
        if let Ok(password) = sigrun_core::askpass::answer(&socket, &id) {
            println!("{password}");
            return 0;
        }
    }
    eprintln!("sigrun: could not read the stored password from the Sigrun host");
    2
}
