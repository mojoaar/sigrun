//! One-shot IPC client used by the CLI to talk to a running `sigrun host`.

use std::path::Path;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt};

use crate::error::{Error, Result};
use crate::ipc::protocol::{self, Request, Response};

/// Send one request to the host at `addr` and return its response.
pub async fn request(addr: &Path, req: &Request) -> Result<Response> {
    #[cfg(unix)]
    {
        let stream = tokio::time::timeout(
            Duration::from_secs(2),
            tokio::net::UnixStream::connect(addr),
        )
        .await
        .map_err(|_| host_not_running())?
        .map_err(|_| host_not_running())?;
        exchange(stream, req).await
    }
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        // `open` is a synchronous `CreateFile` that returns promptly (NotFound
        // or ERROR_PIPE_BUSY), so no async connect timeout applies here.
        let stream = ClientOptions::new()
            .open(addr.as_os_str())
            .map_err(|_| host_not_running())?;
        exchange(stream, req).await
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (addr, req);
        Err(Error::UnsupportedPlatform("ipc client".into()))
    }
}

/// Write one request line and read one response line.
async fn exchange<S>(stream: S, req: &Request) -> Result<Response>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut stream = tokio::io::BufReader::new(stream);

    stream
        .write_all(&protocol::encode(req)?)
        .await
        .map_err(|e| Error::Ipc(e.to_string()))?;
    stream
        .flush()
        .await
        .map_err(|e| Error::Ipc(e.to_string()))?;

    let mut line = Vec::new();
    tokio::time::timeout(Duration::from_secs(30), stream.read_until(b'\n', &mut line))
        .await
        .map_err(|_| Error::Ipc("timed out waiting for the Sigrun host".into()))?
        .map_err(|e| Error::Ipc(e.to_string()))?;

    protocol::decode(&line)
}

fn host_not_running() -> Error {
    Error::Ipc("Sigrun host is not running; start it with: sigrun host".into())
}
