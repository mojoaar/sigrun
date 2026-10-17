//! Live-server check of provisioning against a throwaway host.
//!
//! Ignored by default because it needs a real SSH server to log into. Run:
//!
//! ```text
//! SIGRUN_LIVE_HOST=… SIGRUN_LIVE_USER=… SIGRUN_LIVE_PASSWORD=… SIGRUN_LIVE_KEY=… \
//!   cargo test -p sigrun-core --test live_provision -- --ignored --nocapture
//! ```
//!
//! `SIGRUN_LIVE_KEY` is a path to an unencrypted private key PEM; the host is
//! trusted for this run from `ssh-keyscan` into a throwaway `known_hosts` in a
//! tempdir, so the run never touches your own trust store.

use sigrun_core::provision::{install_keys, ProvisionKey, ProvisionOptions};
use sigrun_core::trust;

fn env_or_skip(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(v) if !v.is_empty() => Some(v),
        _ => {
            println!("skipping live provision test: {name} is not set");
            None
        }
    }
}

/// Trust `host` for one run by recording every key `ssh-keyscan` reports.
fn seed_known_hosts(host: &str, known_hosts: &std::path::Path) -> bool {
    let out = match std::process::Command::new("ssh-keyscan").arg(host).output() {
        Ok(o) if o.status.success() => o,
        _ => {
            println!("skipping live provision test: ssh-keyscan {host} failed");
            return false;
        }
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut seeded = false;
    for line in text.lines() {
        let mut p = line.split_whitespace();
        if let (Some(_), Some(algo), Some(b64)) = (p.next(), p.next(), p.next()) {
            if trust::record(known_hosts, host, 22, algo, b64).is_ok() {
                seeded = true;
            }
        }
    }
    if !seeded {
        println!("skipping live provision test: ssh-keyscan {host} returned no usable keys");
    }
    seeded
}

#[tokio::test]
#[ignore = "needs a live SSH server; see the file header for the run command"]
async fn installs_and_verifies_a_key_on_a_live_host() {
    let Some(host) = env_or_skip("SIGRUN_LIVE_HOST") else {
        return;
    };
    let Some(user) = env_or_skip("SIGRUN_LIVE_USER") else {
        return;
    };
    let Some(password) = env_or_skip("SIGRUN_LIVE_PASSWORD") else {
        return;
    };
    let Some(key_path) = env_or_skip("SIGRUN_LIVE_KEY") else {
        return;
    };

    let dir = tempfile::tempdir().unwrap();
    let known_hosts = dir.path().join("known_hosts");
    if !seed_known_hosts(&host, &known_hosts) {
        return;
    }

    let private_pem = zeroize::Zeroizing::new(std::fs::read_to_string(&key_path).unwrap());
    let public_openssh = russh::keys::PrivateKey::from_openssh(&private_pem)
        .unwrap()
        .public_key()
        .to_openssh()
        .unwrap();
    let key = ProvisionKey {
        name: "live-test".into(),
        public_openssh,
        private_pem,
    };

    let opts = ProvisionOptions {
        host,
        port: 22,
        user,
        password,
        known_hosts,
        disable_password_auth: false,
    };
    let outcome = install_keys(&opts, std::slice::from_ref(&key))
        .await
        .unwrap();

    assert!(
        outcome.installed.iter().any(|n| n == "live-test"),
        "installed did not contain the key: {:?}",
        outcome.installed
    );
    assert_eq!(outcome.verified, vec!["live-test".to_string()]);
    println!(
        "installed: {:?}, verified: {:?}",
        outcome.installed, outcome.verified
    );
}
