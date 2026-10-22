//! A thin wrapper over the core IPC client that turns `Response::Error` into an
//! `anyhow` error (so `main` prints `error: <code>: <message>`).

use std::path::Path;

use sigrun_core::ipc::{Request, Response};

pub async fn request(addr: &Path, req: &Request) -> anyhow::Result<Response> {
    let resp = sigrun_core::ipc::client::request(addr, req).await?;
    match resp {
        Response::Error { code, message, .. } => anyhow::bail!("{code}: {message}"),
        other => Ok(other),
    }
}
