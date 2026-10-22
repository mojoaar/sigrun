//! The host's long-lived IPC listener: serves CLI requests until the host
//! shuts down or a client hangs up.

use std::path::Path;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt};

use crate::error::Result;
use crate::host::{self, HostState};
use crate::ipc::protocol::{self, Request, Response};

/// Serve IPC requests on `addr` until `state.shutdown` is cancelled.
pub async fn serve(addr: &Path, state: Arc<HostState>) -> Result<()> {
    #[cfg(unix)]
    {
        serve_unix(addr, state).await
    }
    #[cfg(windows)]
    {
        serve_windows(addr, state).await
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (addr, state);
        Err(crate::error::Error::UnsupportedPlatform(
            "ipc server".into(),
        ))
    }
}

#[cfg(unix)]
async fn serve_unix(addr: &Path, state: Arc<HostState>) -> Result<()> {
    if addr.exists() {
        match tokio::net::UnixStream::connect(addr).await {
            // A live host is already serving on this socket.
            Ok(_) => return Err(crate::error::Error::AlreadyRunning),
            // A stale socket left by a crashed host: clear it and continue.
            Err(_) => {
                let _ = std::fs::remove_file(addr);
            }
        }
    }
    let listener = tokio::net::UnixListener::bind(addr)?;
    loop {
        tokio::select! {
            _ = state.shutdown.cancelled() => break,
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => {
                        match stream.peer_cred() {
                            Ok(cred) if cred.uid() == unsafe { libc::getuid() } => {}
                            _ => continue,
                        }
                        let state = state.clone();
                        tokio::spawn(handle_conn(stream, state));
                    }
                    Err(_) => continue,
                }
            }
        }
    }
    let _ = std::fs::remove_file(addr);
    Ok(())
}

#[cfg(windows)]
async fn serve_windows(addr: &Path, state: Arc<HostState>) -> Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let name = addr.as_os_str();
    // A named-pipe path must be `\\.\pipe\<name>`; a filesystem path is a
    // caller error. Fail fast instead of spinning in the accept loop below.
    let valid = addr
        .to_str()
        .map(|s| s.starts_with(r"\\.\pipe\"))
        .unwrap_or(false);
    if !valid {
        return Err(crate::error::Error::InvalidInput(format!(
            "ipc socket on Windows must be a named pipe (\\\\.\\pipe\\<name>), got {}",
            addr.display()
        )));
    }
    loop {
        // A second pipe with the same name fails to create (already running).
        let server = match ServerOptions::new().create(name) {
            Ok(server) => server,
            Err(_) => continue,
        };
        tokio::select! {
            _ = state.shutdown.cancelled() => break,
            connected = server.connect() => {
                match connected {
                    Ok(()) => {
                        let state = state.clone();
                        tokio::spawn(handle_conn(server, state));
                    }
                    Err(_) => continue,
                }
            }
        }
    }
    Ok(())
}

/// Serve newline-delimited requests on one connection until EOF.
async fn handle_conn<S>(stream: S, state: Arc<HostState>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = tokio::io::BufReader::new(reader);
    let mut line = Vec::new();

    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line).await {
            Ok(0) => break, // EOF: client went away.
            Ok(_) => {}
            Err(_) => break,
        }

        let req = match protocol::decode::<Request>(&line) {
            Ok(req) => req,
            Err(e) => {
                let resp = Response::Error {
                    code: protocol::error_code(&e).to_string(),
                    message: e.to_string(),
                };
                let data = match protocol::encode(&resp) {
                    Ok(data) => data,
                    Err(_) => break,
                };
                if writer.write_all(&data).await.is_err() || writer.flush().await.is_err() {
                    break;
                }
                continue;
            }
        };

        let resp = host::handle(&state, req).await;
        let data = match protocol::encode(&resp) {
            Ok(data) => data,
            Err(_) => break,
        };
        if writer.write_all(&data).await.is_err() || writer.flush().await.is_err() {
            break;
        }
    }
}
