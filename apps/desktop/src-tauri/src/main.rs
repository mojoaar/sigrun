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
    // The launched line ends by reporting its own session end the same way —
    // no window, tray or host may start for that either.
    if let Some((socket, id)) = connection_ended_args(&args) {
        if let Some(socket) = sigrun_core::connection_ended::helper_socket(socket) {
            tauri::async_runtime::block_on(sigrun_core::connection_ended::report(&socket, &id));
        }
        std::process::exit(0);
    }
    // The same line reports its own shell's pid before `ssh` runs, so the host
    // can notice a session whose terminal is killed outright.
    if let Some((socket, id, pid)) = session_started_args(&args) {
        if let Some(socket) = sigrun_core::session_started::helper_socket(socket) {
            tauri::async_runtime::block_on(sigrun_core::session_started::report(&socket, &id, pid));
        }
        std::process::exit(0);
    }
    sigrun_desktop::run()
}

/// The `connection-ended` shape the launched line passes this binary: the
/// socket it should report to, and the connection id. `None` for every other
/// invocation, including `connection-ended` with no id.
fn connection_ended_args(args: &[String]) -> Option<(Option<std::path::PathBuf>, String)> {
    let rest = args.get(1..)?;
    let at = rest.iter().position(|a| a == "connection-ended")?;
    let id = rest.get(at + 1)?.clone();
    let socket = rest
        .windows(2)
        .find(|w| w[0] == "--socket")
        .map(|w| std::path::PathBuf::from(&w[1]));
    Some((socket, id))
}

/// The `session-started` shape the launched line passes this binary: the socket
/// it should report to, the connection id, and the shell's pid. `None` for every
/// other invocation, including a `session-started` without both values.
fn session_started_args(args: &[String]) -> Option<(Option<std::path::PathBuf>, String, u32)> {
    let rest = args.get(1..)?;
    let at = rest.iter().position(|a| a == "session-started")?;
    let id = rest.get(at + 1)?.clone();
    let pid = rest.get(at + 2)?.parse().ok()?;
    let socket = rest
        .windows(2)
        .find(|w| w[0] == "--socket")
        .map(|w| std::path::PathBuf::from(&w[1]));
    Some((socket, id, pid))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn args(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn connection_ended_args_reads_the_launched_line() {
        assert_eq!(
            connection_ended_args(&args(&[
                "sigrun-desktop",
                "--socket",
                "/tmp/h.sock",
                "connection-ended",
                "c1"
            ])),
            Some((Some(PathBuf::from("/tmp/h.sock")), "c1".to_string()))
        );
        assert_eq!(
            connection_ended_args(&args(&["sigrun-desktop", "connection-ended", "c1"])),
            Some((None, "c1".to_string()))
        );
    }

    #[test]
    fn connection_ended_args_needs_an_id() {
        assert_eq!(
            connection_ended_args(&args(&[
                "sigrun-desktop",
                "--socket",
                "/tmp/h.sock",
                "connection-ended"
            ])),
            None
        );
    }

    #[test]
    fn connection_ended_args_ignores_an_ordinary_launch() {
        assert_eq!(connection_ended_args(&args(&["sigrun-desktop"])), None);
        assert_eq!(
            connection_ended_args(&args(&["sigrun-desktop", "--foo"])),
            None
        );
    }

    #[test]
    fn session_started_args_reads_the_launched_line() {
        assert_eq!(
            session_started_args(&args(&[
                "sigrun-desktop",
                "--socket",
                "/tmp/h.sock",
                "session-started",
                "c1",
                "4242"
            ])),
            Some((Some(PathBuf::from("/tmp/h.sock")), "c1".to_string(), 4242))
        );
        assert_eq!(
            session_started_args(&args(&["sigrun-desktop", "session-started", "c1", "4242"])),
            Some((None, "c1".to_string(), 4242))
        );
    }

    // The line hands this helper `$$`, which the shell has already expanded, so
    // a shape that is missing the pid is not this helper.
    #[test]
    fn session_started_args_needs_an_id_and_a_pid() {
        assert_eq!(
            session_started_args(&args(&["sigrun-desktop", "session-started", "c1"])),
            None
        );
        assert_eq!(
            session_started_args(&args(&["sigrun-desktop", "session-started", "c1", "$$"])),
            None
        );
    }

    #[test]
    fn session_started_args_ignores_an_ordinary_launch() {
        assert_eq!(session_started_args(&args(&["sigrun-desktop"])), None);
        assert_eq!(
            session_started_args(&args(&["sigrun-desktop", "--foo"])),
            None
        );
    }
}
