//! The SSH askpass helper: fetch one connection's stored password from the
//! running host. Synchronous on purpose — ssh runs this binary as a plain
//! program and only its stdout reaches ssh.

use std::io::{BufRead, Read, Write};
use std::path::Path;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::ipc::protocol::{self, Request, Response};

/// How long to wait for the host before giving up (matching the IPC client).
const TIMEOUT: Duration = Duration::from_secs(30);

/// A fixed reason for every failure: no socket, no host, a bad response, or a
/// locked vault. It never carries any part of a password.
fn unavailable() -> Error {
    Error::Ipc("could not read the stored password from the Sigrun host".into())
}

/// Ask the host at `socket` for `connection_id`'s password.
///
/// The returned string is the only secret this crate hands out to a caller;
/// nothing here logs, formats, or errors with it.
pub fn answer(socket: &Path, connection_id: &str) -> Result<String> {
    let req = Request::PasswordGet {
        id: connection_id.to_string(),
    };
    let line = exchange(socket, &req)?;
    let resp: Response = protocol::decode(&line).map_err(|_| unavailable())?;
    match resp {
        // An empty password would only make ssh fail after one attempt with a
        // bare newline on the terminal, so treat it like any other failure.
        Response::Password(password) if !password.is_empty() => Ok(password),
        _ => Err(unavailable()),
    }
}

#[cfg(unix)]
fn exchange(socket: &Path, req: &Request) -> Result<Vec<u8>> {
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket).map_err(|_| unavailable())?;
    stream.set_read_timeout(Some(TIMEOUT)).ok();
    stream.set_write_timeout(Some(TIMEOUT)).ok();
    roundtrip(&mut stream, req)
}

#[cfg(windows)]
fn exchange(socket: &Path, req: &Request) -> Result<Vec<u8>> {
    use std::fs::OpenOptions;

    // A pipe the host just served may not have its next instance yet, so retry
    // briefly (ERROR_PIPE_BUSY / no such file) instead of reporting it down.
    let deadline = std::time::Instant::now() + TIMEOUT;
    let mut stream = loop {
        match OpenOptions::new().read(true).write(true).open(socket) {
            Ok(file) => break file,
            Err(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return Err(unavailable()),
        }
    };
    roundtrip(&mut stream, req)
}

#[cfg(not(any(unix, windows)))]
fn exchange(_socket: &Path, _req: &Request) -> Result<Vec<u8>> {
    Err(crate::error::Error::UnsupportedPlatform("askpass".into()))
}

/// Write one request line and read one response line, without a runtime.
fn roundtrip<S: Read + Write>(stream: &mut S, req: &Request) -> Result<Vec<u8>> {
    stream
        .write_all(&protocol::encode(req).map_err(|_| unavailable())?)
        .map_err(|_| unavailable())?;
    stream.flush().map_err(|_| unavailable())?;

    let mut line = Vec::new();
    std::io::BufReader::new(stream)
        .read_until(b'\n', &mut line)
        .map_err(|_| unavailable())?;
    if line.is_empty() {
        return Err(unavailable());
    }
    Ok(line)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// Accept one connection on `sock`, assert the request, and answer with
    /// `reply`. Returns the accept thread so the test can join it.
    fn host_reply(sock: &Path, reply: Response) -> std::thread::JoinHandle<()> {
        let listener = std::os::unix::net::UnixListener::bind(sock).unwrap();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream);
            let mut line = Vec::new();
            reader.read_until(b'\n', &mut line).unwrap();
            let req: Request = protocol::decode(&line).unwrap();
            assert_eq!(req, Request::PasswordGet { id: "c1".into() });
            reader
                .get_mut()
                .write_all(&protocol::encode(&reply).unwrap())
                .unwrap();
        })
    }

    const FIXED: &str = "ipc error: could not read the stored password from the Sigrun host";

    #[test]
    fn asks_the_host_and_returns_only_the_password() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("host.sock");
        let server = host_reply(&sock, Response::Password("pw-123".into()));
        assert_eq!(answer(&sock, "c1").unwrap(), "pw-123");
        server.join().unwrap();
    }

    #[test]
    fn an_empty_password_is_a_failure() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("host.sock");
        // What the host sends when it has an empty value: ssh must never be
        // handed it as a success.
        let server = host_reply(&sock, Response::Password(String::new()));
        let err = answer(&sock, "c1").unwrap_err();
        assert_eq!(err.to_string(), FIXED);
        server.join().unwrap();
    }

    #[test]
    fn a_host_error_is_a_failure() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("host.sock");
        let server = host_reply(
            &sock,
            Response::Error {
                code: "not_found".into(),
                message: "not found: c1".into(),
                payload: None,
            },
        );
        let err = answer(&sock, "c1").unwrap_err();
        assert_eq!(err.to_string(), FIXED);
        server.join().unwrap();
    }

    #[test]
    fn a_missing_host_fails_with_a_fixed_reason() {
        let dir = tempfile::tempdir().unwrap();
        let err = answer(&dir.path().join("missing.sock"), "c1").unwrap_err();
        assert_eq!(err.to_string(), FIXED);
    }
}
