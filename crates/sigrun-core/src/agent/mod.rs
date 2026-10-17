//! Sigrun's SSH agent: an OpenSSH-compatible in-memory agent that serves
//! vault-held private keys so `ssh` can use them without any plaintext key
//! ever touching disk.

use crate::error::{Error, Result};

pub mod session;

pub use session::{sign_raw, vault_keys, AgentKey, AgentState, SigrunAgent};

/// Serve the OpenSSH agent protocol on `addr` (a Unix socket file or a Windows
/// named pipe) using the keys held in `state`.
///
/// Returns a [`tokio::task::JoinHandle`] the host can `abort` on shutdown. The
/// accept loop never returns on its own; a terminal error inside it is
/// discarded so the handle stays `JoinHandle<()>`.
pub async fn serve(
    addr: &std::path::Path,
    state: AgentState,
) -> Result<tokio::task::JoinHandle<()>> {
    let agent = SigrunAgent::new(state);

    #[cfg(unix)]
    let handle = {
        use std::os::unix::fs::PermissionsExt;

        // A stale socket left by a crashed host is not an error.
        match std::fs::remove_file(addr) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::Io(e)),
        }
        let listener = tokio::net::UnixListener::bind(addr)?;
        std::fs::set_permissions(addr, std::fs::Permissions::from_mode(0o600))?;

        tokio::spawn(async move {
            let _ = ssh_agent_lib::agent::listen(listener, agent).await;
        })
    };

    #[cfg(windows)]
    let handle = {
        // Named pipes have no on-disk file to chmod; the name is already
        // per-user.
        let listener = ssh_agent_lib::agent::NamedPipeListener::bind(addr.as_os_str())?;
        tokio::spawn(async move {
            let _ = ssh_agent_lib::agent::listen(listener, agent).await;
        })
    };

    #[cfg(not(any(unix, windows)))]
    let handle: tokio::task::JoinHandle<()> =
        return Err(Error::UnsupportedPlatform("agent socket".into()));

    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::generate::generate;
    use crate::model::KeyAlgorithm;
    use ssh_agent_lib::agent::Session;

    #[cfg(unix)]
    #[tokio::test]
    async fn serves_identities_over_a_unix_socket() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let addr = dir.path().join("agent.sock");
        let state = AgentState::new();
        let k = generate(KeyAlgorithm::Ed25519, None).unwrap();
        let key = ssh_key::PrivateKey::from_openssh(&k.private_pem).unwrap();
        state.replace(vec![AgentKey {
            key_id: "k1".into(),
            private_key: key,
            comment: "k1".into(),
        }]);

        let handle = serve(&addr, state).await.unwrap();
        assert_eq!(
            std::fs::metadata(&addr).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let stream = tokio::net::UnixStream::connect(&addr).await.unwrap();
        let mut client = ssh_agent_lib::client::Client::new(stream);
        let ids = client.request_identities().await.unwrap();
        assert_eq!(ids.len(), 1);
        handle.abort();
    }
}
