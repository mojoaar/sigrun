//! The `connection-ended` helper: the last fragment of a launched line reports
//! its own session back to the host. Runs in the user's terminal after `ssh`
//! exits, so it never prints and never fails — an already-gone host, or a
//! message the user cannot act on, must not scroll into their session.

use std::path::{Path, PathBuf};

use crate::ipc::client;
use crate::ipc::Request;
use crate::paths::Paths;

/// The socket a helper launched from a terminal should talk to: the explicit
/// `--socket` the launched line passes, then `SIGRUN_SOCKET`, then the default
/// location. `None` when there is no home to resolve a default under.
pub fn helper_socket(explicit: Option<PathBuf>) -> Option<PathBuf> {
    explicit
        .or_else(|| std::env::var_os("SIGRUN_SOCKET").map(PathBuf::from))
        .or_else(|| Paths::resolve().ok().map(|p| p.socket_path()))
}

/// Tell the host at `socket` that `id`'s session ended. The outcome is
/// discarded on purpose: only the id and the socket path are involved, and the
/// process's exit status is ignored by the line that launched it.
pub async fn report(socket: &Path, id: &str) {
    let _ = client::request(socket, &Request::ConnectionEnded { id: id.to_string() }).await;
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
    async fn report_asks_the_host_to_end_the_connection() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("host.sock");
        // An answer the caller must swallow: the session is over either way.
        let server = host_reply(
            &sock,
            Response::Error {
                code: "not_found".into(),
                message: "not found: c1".into(),
                payload: None,
            },
        );
        report(&sock, "c1").await;
        assert_eq!(
            server.join().unwrap(),
            Request::ConnectionEnded { id: "c1".into() }
        );
    }

    #[tokio::test]
    async fn a_missing_host_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        report(&dir.path().join("missing.sock"), "c1").await;
    }

    #[test]
    fn an_explicit_socket_wins() {
        let explicit = PathBuf::from("/tmp/explicit.sock");
        assert_eq!(helper_socket(Some(explicit.clone())), Some(explicit));
    }
}
