//! Live-server check of provisioning against a throwaway host.
//!
//! Ignored by default because it needs a real SSH server to log into. Run:
//!
//! ```text
//! SIGRUN_LIVE_HOST=… SIGRUN_LIVE_USER=… SIGRUN_LIVE_PASSWORD=… SIGRUN_LIVE_KEY=… \
//!   cargo test -p sigrun-core --test live_provision -- --ignored --nocapture
//! ```
//!
//! To run only the hardening test (it turns password auth off on that host):
//!
//! ```text
//! SIGRUN_LIVE_HOST=… SIGRUN_LIVE_USER=… SIGRUN_LIVE_PASSWORD=… SIGRUN_LIVE_KEY=… \
//!   cargo test -p sigrun-core --test live_provision hardening -- --ignored --nocapture
//! ```
//!
//! `SIGRUN_LIVE_KEY` is a path to an unencrypted private key PEM; the host is
//! trusted for this run from `ssh-keyscan` into a throwaway `known_hosts` in a
//! tempdir, so the run never touches your own trust store.

use sigrun_core::provision::{install_keys, Hardening, ProvisionKey, ProvisionOptions};
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

/// Env-driven options and key. The tempdir owns the throwaway `known_hosts`, so
/// it is returned to keep it alive for the test's lifetime.
fn live_setup() -> Option<(tempfile::TempDir, ProvisionOptions, ProvisionKey)> {
    let host = env_or_skip("SIGRUN_LIVE_HOST")?;
    let user = env_or_skip("SIGRUN_LIVE_USER")?;
    let password = env_or_skip("SIGRUN_LIVE_PASSWORD")?;
    let key_path = env_or_skip("SIGRUN_LIVE_KEY")?;

    let dir = tempfile::tempdir().unwrap();
    let known_hosts = dir.path().join("known_hosts");
    if !seed_known_hosts(&host, &known_hosts) {
        return None;
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
    Some((dir, opts, key))
}

#[tokio::test]
#[ignore = "needs a live SSH server; see the file header for the run command"]
async fn installs_and_verifies_a_key_on_a_live_host() {
    let Some((_dir, opts, key)) = live_setup() else {
        return;
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

#[tokio::test]
#[ignore = "needs a live SSH server and passwordless sudo; disables password auth on it"]
async fn hardening_is_applied_on_a_live_host() {
    let Some((_dir, mut opts, key)) = live_setup() else {
        return;
    };
    opts.disable_password_auth = true;
    let outcome = install_keys(&opts, std::slice::from_ref(&key))
        .await
        .unwrap();

    assert_eq!(outcome.hardening, Hardening::Applied);
    println!("hardening applied on {}", opts.host);
}
