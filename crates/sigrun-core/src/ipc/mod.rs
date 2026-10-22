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
        // On Windows the IPC socket is a named pipe (`\\.\pipe\…`), not a file;
        // a filesystem path makes the server reject it. Use a per-test pipe
        // name there, a filesystem path elsewhere.
        let sock = if cfg!(windows) {
            std::path::PathBuf::from(format!(
                r"\\.\pipe\sigrun-ipc-test-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
        } else {
            dir.path().join("host.sock")
        };
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

    #[tokio::test]
    async fn provisioning_unknown_connection_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::with_roots(dir.path().into(), dir.path().into(), dir.path().join("ssh"));
        let state = HostState::new(paths).await.unwrap();
        let resp = handle(
            &state,
            Request::ConnectionsProvision {
                id: "missing".into(),
                password: "x".into(),
                key_ids: vec![],
                disable_password_auth: false,
            },
        )
        .await;
        match resp {
            Response::Error { code, .. } => assert_eq!(code, "not_found"),
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn provisioning_a_locked_vault_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::with_roots(dir.path().into(), dir.path().into(), dir.path().join("ssh"));
        let state = HostState::new(paths).await.unwrap();
        let mut c = crate::model::Connection::managed("c1", "web");
        c.host = "example.com".into();
        c.user = "deploy".into();
        state.store.upsert_connection(c).unwrap();
        let resp = handle(
            &state,
            Request::ConnectionsProvision {
                id: "c1".into(),
                password: "x".into(),
                key_ids: vec!["k1".into()],
                disable_password_auth: false,
            },
        )
        .await;
        match resp {
            Response::Error { code, .. } => assert_eq!(code, "vault_locked"),
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }

    #[tokio::test]
    async fn connect_refuses_an_unreachable_host_unless_forced() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::with_roots(dir.path().into(), dir.path().into(), dir.path().join("ssh"));
        let state = HostState::new(paths).await.unwrap();
        let mut c = Connection::managed("c1", "web");
        c.host = "127.0.0.1".into();
        // Port 1 is closed on every host we run on, so the probe refuses it.
        c.port = 1;
        c.user = "u".into();
        state.store.upsert_connection(c.clone()).unwrap();

        // The TOFU gate runs before the reachability probe; with no known_hosts
        // entry and no keyscan answer, it fails closed as a probe failure.
        let resp = handle(
            &state,
            Request::ConnectionsConnect {
                id: "c1".into(),
                print_only: false,
                force: false,
            },
        )
        .await;
        match resp {
            Response::Error { code, message, .. } => {
                assert_eq!(code, "invalid_config");
                assert!(
                    message.contains("could not scan host keys"),
                    "message was {message:?}"
                );
            }
            other => panic!("unexpected {other:?}"),
        }

        // Forced, the gate is skipped: with an impossible port the check in
        // `ssh_invocation` refuses next, so the terminal launcher is never
        // reached.
        c.port = 0;
        state.store.upsert_connection(c).unwrap();
        let resp = handle(
            &state,
            Request::ConnectionsConnect {
                id: "c1".into(),
                print_only: false,
                force: true,
            },
        )
        .await;
        match resp {
            Response::Error { code, .. } => assert_eq!(code, "invalid_input"),
            other => panic!("unexpected {other:?}"),
        }
        state.shutdown.cancel();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unix_socket_verifies_peer_credentials() {
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

        let res = client::request(&sock, &Request::Status).await;
        assert!(res.is_ok());
        state.shutdown.cancel();
        let _ = server.await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn oversized_request_line_is_rejected_without_unbounded_buffering() {
        use super::protocol;
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

        // Send a stream of MAX_LINE + 1024 bytes with no newline
        use tokio::io::AsyncWriteExt;
        let mut stream = tokio::net::UnixStream::connect(&sock).await.unwrap();
        let garbage = vec![b'a'; protocol::MAX_LINE + 1024];
        let res = stream.write_all(&garbage).await;
        assert!(res.is_ok());

        // Connection should be closed by server when line exceeds MAX_LINE
        use tokio::io::AsyncReadExt;
        let mut resp = Vec::new();
        let read_res = stream.read_to_end(&mut resp).await;
        assert!(read_res.is_ok());

        state.shutdown.cancel();
        let _ = server.await;
    }

    #[tokio::test]
    async fn settings_roundtrip_over_ipc() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::with_roots(dir.path().into(), dir.path().into(), dir.path().into());
        let state = HostState::new(paths).await.unwrap();

        // Initially returns default settings
        let resp = handle(&state, Request::SettingsGet).await;
        let mut settings = match resp {
            Response::Settings(s) => s,
            other => panic!("expected Response::Settings, got {other:?}"),
        };
        assert_eq!(settings.theme, "nordlys");

        // Mutate settings and save
        settings.theme = "frost".into();
        let resp = handle(
            &state,
            Request::SettingsSet {
                settings: Box::new(settings.clone()),
            },
        )
        .await;
        assert!(matches!(resp, Response::Ok));

        // Read back and verify mutation persisted
        let resp = handle(&state, Request::SettingsGet).await;
        match resp {
            Response::Settings(s) => assert_eq!(s.theme, "frost"),
            other => panic!("expected Response::Settings, got {other:?}"),
        }

        state.shutdown.cancel();
    }
}
