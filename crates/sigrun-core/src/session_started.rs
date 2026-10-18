//! The `session-started` helper: the fragment of a launched line that hands the
//! host the pid of the shell running `ssh`. The host watches it because a
//! terminal that kills the whole process group (closing a Ghostty tab) runs no
//! handler inside that shell, so the end report never comes. Runs in the user's
//! terminal, so it never prints and never fails.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::ipc::client;
use crate::ipc::Request;

/// How long the terminal waits for the host to acknowledge the report. The
/// report runs before `ssh` and the client's own response wait is 30 seconds,
/// so a host that accepts the socket but never answers must not stall a connect.
const REPORT_TIMEOUT: Duration = Duration::from_secs(2);

/// The socket a helper launched from a terminal should talk to: the explicit
/// `--socket` the launched line passes, then `SIGRUN_SOCKET`, then the default
/// location. Resolved exactly as `connection_ended` resolves it.
pub fn helper_socket(explicit: Option<PathBuf>) -> Option<PathBuf> {
    crate::connection_ended::helper_socket(explicit)
}

/// Tell the host at `socket` that `id`'s session is running as `pid`. The
/// outcome is discarded on purpose: only the id, the pid and the socket path are
/// involved, and the process's exit status is ignored by the line that launched
/// it. Bounded by [`REPORT_TIMEOUT`]: a host that never answers reads as no
/// report, which leaves the pid unwatched exactly as if it were not there.
pub async fn report(socket: &Path, id: &str, pid: u32) {
    let _ = tokio::time::timeout(
        REPORT_TIMEOUT,
        client::request(
            socket,
            &Request::SessionStarted {
                id: id.to_string(),
                pid,
            },
        ),
    )
    .await;
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::ipc::{decode, encode, Response};
    use std::io::{BufRead, BufReader, Write};

    /// Answer one connection's request with `reply`, returning the accepted
    /// request for the test to check.
    fn host_reply(sock: &Path, reply: Response) -> std::thread::JoinHandle<Request> {
        let listener = std::os::unix::net::UnixListener::bind(sock).unwrap();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = Vec::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_until(b'\n', &mut line)
                .unwrap();
            let req = decode(&line).unwrap();
            let mut stream = stream;
            stream.write_all(&encode(&reply).unwrap()).unwrap();
            stream.flush().unwrap();
            req
        })
    }

    #[tokio::test]
    async fn report_hands_the_host_the_shells_pid() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("host.sock");
        let server = host_reply(&sock, Response::Ok);
        report(&sock, "c1", 4242).await;
        assert_eq!(
            server.join().unwrap(),
            Request::SessionStarted {
                id: "c1".into(),
                pid: 4242
            }
        );
    }

    #[tokio::test]
    async fn a_missing_host_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        report(&dir.path().join("missing.sock"), "c1", 1).await;
    }

    #[tokio::test]
    async fn a_host_that_never_replies_does_not_stall_the_report() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("host.sock");
        let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        // Accept and then sit on the connection without ever answering, the way
        // a wedged host would. A bare accept is enough: the report is best-effort
        // and must not wait out the client's 30 s response timeout.
        std::thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_secs(30));
        });
        let start = std::time::Instant::now();
        report(&sock, "c1", 1).await;
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn an_explicit_socket_wins() {
        let explicit = PathBuf::from("/tmp/explicit.sock");
        assert_eq!(helper_socket(Some(explicit.clone())), Some(explicit));
    }
}
