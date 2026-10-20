//! End-to-end cross-module test: generate a key, seal it in the vault, load it
//! into the agent, and build the `ssh` argv for a connection that uses it.

use sigrun_core::agent;
use sigrun_core::keys;
use sigrun_core::model::{effective, Connection, KeyAlgorithm, KeyMetadata, KeySource};
use sigrun_core::paths::Paths;
use sigrun_core::store::Store;
use sigrun_core::terminal;
use sigrun_core::vault::{Secret, Vault};

#[tokio::test]
async fn vault_to_agent_to_connection_flow() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::with_roots(dir.path().into(), dir.path().into(), dir.path().into());
    let store = Store::new(paths.clone());
    let vault = Vault::new(paths.vault_file());
    vault.create("pass").unwrap();
    let mut v = vault.unlock("pass").unwrap();

    let generated = keys::generate(KeyAlgorithm::Ed25519, None).unwrap();
    v.put(
        "k1",
        Secret::PrivateKey {
            pem: generated.private_pem,
            passphrase: None,
        },
    )
    .unwrap();
    let meta = KeyMetadata::now(
        "k1",
        KeyAlgorithm::Ed25519,
        generated.public_openssh,
        generated.fingerprint,
        KeySource::Vault,
    );
    store.upsert_key(meta.clone()).unwrap();

    let (agent_keys, warnings) = agent::vault_keys(&v, &[meta]);
    assert!(warnings.is_empty());
    assert_eq!(agent_keys.len(), 1);

    let mut conn = Connection::managed("c1", "web");
    conn.host = "example.com".into();
    conn.user = "deploy".into();
    conn.key_id = Some("k1".into());
    store.upsert_connection(conn.clone()).unwrap();

    let data = store.load_data().unwrap();
    let auth = effective(&conn, &data.credentials).unwrap();
    let argv = terminal::ssh_invocation(&conn, &auth, data.keys.first()).unwrap();
    assert_eq!(argv, vec!["ssh", "deploy@example.com", "-p", "22"]);
}
