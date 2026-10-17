pub mod client;
pub mod protocol;
pub mod server;

pub use protocol::{decode, encode, error_code, Request, Response, StatusInfo};

#[cfg(test)]
mod tests {
    use super::client;
    use super::protocol::{Request, Response};
    use super::server::serve;
    use crate::error::Error;
    use crate::host::{handle, sync_disk_keys, HostState};
    use crate::keys::generate;
    use crate::model::{Connection, KeyAlgorithm};
    use crate::paths::Paths;
    use crate::store::Store;

    #[tokio::test]
    async fn status_and_vault_roundtrip_over_ipc() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("host.sock");
        let paths = Paths::with_roots(dir.path().into(), dir.path().into(), dir.path().into());
        let state = std::sync::Arc::new(HostState::new(paths).await.unwrap());
        let server = tokio::spawn({
            let state = state.clone();
            let sock = sock.clone();
            async move { serve(&sock, state).await }
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let r = client::request(
            &sock,
            &Request::VaultInit {
                passphrase: "pass".into(),
            },
        )
        .await
        .unwrap();
        assert!(matches!(r, Response::Ok));
        let r = client::request(
            &sock,
            &Request::VaultUnlock {
                passphrase: "pass".into(),
            },
        )
        .await
        .unwrap();
        assert!(matches!(r, Response::Unlocked { .. }));

        state.shutdown.cancel();
        let _ = server.await;
    }

    #[tokio::test]
    async fn second_host_refuses_to_start() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::with_roots(dir.path().into(), dir.path().into(), dir.path().into());
        let first = std::sync::Arc::new(HostState::new(paths.clone()).await.unwrap());
        assert!(matches!(
            HostState::new(paths).await,
            Err(Error::AlreadyRunning)
        ));
        first.shutdown.cancel();
    }

    #[tokio::test]
    async fn sync_includes_keys_referenced_outside_the_ssh_dir() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join("ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        let outside = dir.path().join("id_external");
        let k = generate(KeyAlgorithm::Ed25519, None).unwrap();
        std::fs::write(&outside, &k.private_pem).unwrap();
        std::fs::write(dir.path().join("id_external.pub"), &k.public_openssh).unwrap();
        std::fs::write(
            ssh.join("config"),
            format!(
                "Host ext\n  HostName ext.example\n  IdentityFile {}\n",
                outside.display()
            ),
        )
        .unwrap();
        let store = Store::new(Paths::with_roots(
            dir.path().into(),
            dir.path().into(),
            ssh.clone(),
        ));
        let views = sync_disk_keys(&store, &ssh).unwrap();
        assert!(views
            .iter()
            .any(|v| v.key.disk_path.as_deref() == Some(outside.to_str().unwrap())));
    }

    #[tokio::test]
    async fn handlers_mutate_state_without_deadlocking() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::with_roots(dir.path().into(), dir.path().into(), dir.path().into());
        let state = std::sync::Arc::new(HostState::new(paths).await.unwrap());

        let out = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            handle(
                &state,
                Request::ConnectionsAdd {
                    connection: Connection::managed("c1", "web"),
                },
            ),
        )
        .await
        .expect("handle timed out — the host lock deadlocked against the store lock");
        assert!(matches!(out, Response::Ok));

        let out = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            handle(&state, Request::ConnectionsRemove { id: "c1".into() }),
        )
        .await
        .expect("handle timed out — the host lock deadlocked against the store lock");
        assert!(matches!(out, Response::Ok));
    }
}
