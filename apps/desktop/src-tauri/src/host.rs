use std::sync::Arc;

use sigrun_core::error::Result;
use sigrun_core::host::HostState;
use sigrun_core::ipc::protocol::Request;
use sigrun_core::paths::Paths;

pub async fn start_host(paths: Paths) -> Result<Arc<HostState>> {
    let state = Arc::new(HostState::new(paths.clone()).await?);
    // Handles are detached: IPC exits on shutdown.cancel(), the agent dies with the process.
    let _agent = sigrun_core::agent::serve(&paths.agent_socket(), state.agent.clone()).await?;
    let ipc_state = state.clone();
    let addr = paths.socket_path();
    let ipc_addr = addr.clone();
    tokio::spawn(async move {
        if let Err(e) = sigrun_core::ipc::server::serve(&ipc_addr, ipc_state).await {
            eprintln!("sigrun: IPC server stopped: {e}");
        }
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if sigrun_core::ipc::client::request(&addr, &Request::Status)
            .await
            .is_ok()
        {
            break;
        }
        if std::time::Instant::now() >= deadline {
            state.shutdown.cancel();
            return Err(sigrun_core::error::Error::Ipc(
                "IPC server did not start".into(),
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn start_host_serves_ipc_and_locks_single_instance() {
        let dir = tempfile::tempdir().unwrap();
        let paths = sigrun_core::paths::Paths::with_roots(
            dir.path().into(),
            dir.path().into(),
            dir.path().join("ssh"),
        );
        let state = start_host(paths.clone()).await.unwrap();
        let resp = sigrun_core::ipc::client::request(&paths.socket_path(), &Request::Status)
            .await
            .unwrap();
        assert!(matches!(
            resp,
            sigrun_core::ipc::protocol::Response::Status(_)
        ));
        // A second start against the same paths must refuse (single host).
        assert!(matches!(
            start_host(paths).await,
            Err(sigrun_core::error::Error::AlreadyRunning)
        ));
        state.shutdown.cancel();
    }
}
