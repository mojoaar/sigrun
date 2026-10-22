//! Trust-on-first-use host-key store (Sigrun-owned `known_hosts`).
//! Line format: `host:port algo key_b64`.
use std::path::Path;

use crate::error::{Error, Result};
use crate::paths::{ensure_private_dir, write_atomic};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustStatus {
    Trusted,
    Unknown,
    Mismatch,
}

fn read(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.into()),
    }
}

fn validate(host: &str, algo: &str, key_b64: &str) -> Result<()> {
    for (name, v) in [("host", host), ("algo", algo), ("key", key_b64)] {
        if v.is_empty() || v.chars().any(|c| c.is_ascii_whitespace() || c.is_control()) {
            return Err(Error::InvalidInput(format!("invalid {name}")));
        }
    }
    Ok(())
}

pub fn check(path: &Path, host: &str, port: u16, algo: &str, key_b64: &str) -> Result<TrustStatus> {
    validate(host, algo, key_b64)?;
    let id = format!("{host}:{port}");
    for line in read(path)?.lines() {
        let mut p = line.split_whitespace();
        if let (Some(h), Some(a), Some(k)) = (p.next(), p.next(), p.next()) {
            // A host may be known by one key type and be asked for another (a client
            // picks its own preference). Only the same type can contradict the key.
            if h == id && a == algo {
                return Ok(if k == key_b64 {
                    TrustStatus::Trusted
                } else {
                    TrustStatus::Mismatch
                });
            }
        }
    }
    Ok(TrustStatus::Unknown)
}

pub fn record(path: &Path, host: &str, port: u16, algo: &str, key_b64: &str) -> Result<()> {
    validate(host, algo, key_b64)?;
    if fingerprint(algo, key_b64).is_none() {
        return Err(Error::InvalidInput("unparseable public key".into()));
    }
    let line = format!("{host}:{port} {algo} {key_b64}");
    let mut content = read(path)?;
    if content.lines().any(|l| l == line) {
        return Ok(());
    }
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&line);
    content.push('\n');
    if let Some(dir) = path.parent() {
        ensure_private_dir(dir)?;
    }
    write_atomic(path, content.as_bytes(), 0o600)
}

pub fn fingerprint(algo: &str, key_b64: &str) -> Option<String> {
    let public = ssh_key::PublicKey::from_openssh(&format!("{algo} {key_b64}")).ok()?;
    Some(public.fingerprint(Default::default()).to_string())
}

/// Run `ssh-keyscan` against `host`:`port` and return the offered keys as
/// `(algo, key_b64)` pairs. Returns `None` when the scan fails or yields no keys.
pub async fn probe_host_key(host: &str, port: u16) -> Option<Vec<(String, String)>> {
    let mut cmd = tokio::process::Command::new("ssh-keyscan");
    cmd.args([
        "-p",
        &port.to_string(),
        "-T",
        "5",
        "-t",
        "ed25519,ecdsa,rsa",
        "--",
        host,
    ])
    .stdin(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .kill_on_drop(true);
    let out = tokio::time::timeout(std::time::Duration::from_secs(5), cmd.output())
        .await
        .ok()?
        .ok()?;
    let keys = parse_keyscan(&String::from_utf8_lossy(&out.stdout));
    (!keys.is_empty()).then_some(keys)
}

fn parse_keyscan(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut p = l.split_whitespace();
            let (_, algo, key) = (p.next()?, p.next()?, p.next()?);
            Some((algo.to_string(), key.to_string()))
        })
        .collect()
}

/// The TOFU gate: evaluate a scanned host key against the known_hosts store.
///
/// A recorded key presented by the host wins (extra unrecorded key types are not a
/// conflict); otherwise any mismatch refuses; otherwise all-unknown prompts.
///
/// Returns structured errors so the caller can surface fingerprint, algorithm and
/// raw key without parsing a message string.
pub fn evaluate_trust(kh: &Path, host: &str, port: u16, keys: &[(String, String)]) -> Result<()> {
    let mut mismatch = None;
    for (algo, key) in keys {
        match check(kh, host, port, algo, key)? {
            TrustStatus::Trusted => return Ok(()),
            TrustStatus::Mismatch => mismatch = mismatch.or(Some((algo, key))),
            TrustStatus::Unknown => {}
        }
    }
    if let Some((algo, key)) = mismatch {
        let fp = fingerprint(algo, key).unwrap_or_else(|| "unknown".into());
        return Err(Error::HostKeyMismatch {
            fingerprint: fp,
            algorithm: algo.to_string(),
            key: key.to_string(),
        });
    }
    match keys.first() {
        Some((algo, key)) => {
            let fp = fingerprint(algo, key).unwrap_or_else(|| "unknown".into());
            Err(Error::HostKeyUnknown {
                fingerprint: fp,
                algorithm: algo.to_string(),
                key: key.to_string(),
            })
        }
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIKKEEWQJy/IdduM5HglENeKuk+5Am8lcPpdlUdvxDWqq";

    #[test]
    fn host_key_trust_is_tofu_and_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let kh = dir.path().join("known_hosts");
        assert_eq!(
            check(&kh, "h.example", 22, "ssh-ed25519", KEY).unwrap(),
            TrustStatus::Unknown
        );
        record(&kh, "h.example", 22, "ssh-ed25519", KEY).unwrap();
        assert_eq!(
            check(&kh, "h.example", 22, "ssh-ed25519", KEY).unwrap(),
            TrustStatus::Trusted
        );
        assert_eq!(
            check(&kh, "h.example", 22, "ssh-ed25519", "BBBB").unwrap(),
            TrustStatus::Mismatch
        );
    }

    #[test]
    fn another_key_type_is_a_first_use_prompt_not_a_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let kh = dir.path().join("known_hosts");
        record(&kh, "h.example", 22, "ssh-ed25519", KEY).unwrap();
        // The same host, asked for a key type it never offered before: this is a
        // trust prompt, not evidence that the host's identity changed.
        assert_eq!(
            check(&kh, "h.example", 22, "ssh-rsa", KEY).unwrap(),
            TrustStatus::Unknown
        );
        assert_eq!(
            check(&kh, "h.example", 22, "ssh-ed25519", KEY).unwrap(),
            TrustStatus::Trusted
        );
        // Only a different key of a type already known is a mismatch.
        assert_eq!(
            check(&kh, "h.example", 22, "ssh-ed25519", "BBBB").unwrap(),
            TrustStatus::Mismatch
        );
    }

    #[test]
    fn record_is_idempotent_and_private() {
        let dir = tempfile::tempdir().unwrap();
        let kh = dir.path().join("sub").join("known_hosts");
        record(&kh, "h", 22, "ssh-ed25519", KEY).unwrap();
        record(&kh, "h", 22, "ssh-ed25519", KEY).unwrap();
        assert_eq!(std::fs::read_to_string(&kh).unwrap().lines().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&kh).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn record_rejects_malformed_input() {
        let dir = tempfile::tempdir().unwrap();
        let kh = dir.path().join("known_hosts");
        assert!(record(&kh, "h\nx", 22, "ssh-ed25519", KEY).is_err());
        assert!(record(&kh, "h", 22, "ssh-ed25519\nx", KEY).is_err());
        assert!(record(&kh, "h", 22, "ssh-ed25519", "AAAA").is_err());
        assert!(check(&kh, "h x", 22, "ssh-ed25519", KEY).is_err());
        assert!(!kh.exists());
    }

    #[test]
    fn parses_all_keyscan_lines() {
        let out = "# h:22 SSH-2.0-x\nh ssh-ed25519 AAAA\nh ssh-rsa ZZZZ\n";
        assert_eq!(
            parse_keyscan(out),
            vec![
                ("ssh-ed25519".into(), "AAAA".into()),
                ("ssh-rsa".into(), "ZZZZ".into())
            ]
        );
        assert!(parse_keyscan("# only comment\n").is_empty());
    }

    #[test]
    fn multi_key_trust_precedence() {
        let dir = tempfile::tempdir().unwrap();
        let kh = dir.path().join("known_hosts");
        record(&kh, "h", 22, "ssh-ed25519", KEY).unwrap();
        let k = |a: &str, b: &str| (a.to_string(), b.to_string());
        let ok = [k("ssh-rsa", "ZZZZ"), k("ssh-ed25519", KEY)];
        assert!(evaluate_trust(&kh, "h", 22, &ok).is_ok());
        let bad = [k("ssh-rsa", "ZZZZ"), k("ssh-ed25519", "BBBB")];
        match evaluate_trust(&kh, "h", 22, &bad).unwrap_err() {
            Error::HostKeyMismatch {
                fingerprint,
                algorithm,
                key,
            } => {
                assert_eq!(algorithm, "ssh-ed25519");
                assert_eq!(key, "BBBB");
                assert_eq!(fingerprint, "unknown");
            }
            other => panic!("unexpected {other:?}"),
        }
        let fresh = [k("ssh-ed25519", KEY)];
        match evaluate_trust(&kh, "other", 22, &fresh).unwrap_err() {
            Error::HostKeyUnknown {
                fingerprint,
                algorithm,
                key,
            } => {
                assert_eq!(algorithm, "ssh-ed25519");
                assert_eq!(key, KEY);
                assert!(fingerprint.starts_with("SHA256:"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn probe_host_key_returns_none_on_failure() {
        // Port 0 is invalid; ssh-keyscan will fail.
        assert!(probe_host_key("127.0.0.1", 0).await.is_none());
    }

    #[test]
    fn evaluate_trust_returns_unknown_for_unknown_host() {
        let dir = tempfile::tempdir().unwrap();
        let kh = dir.path().join("known_hosts");
        let keys = [("ssh-ed25519".into(), KEY.to_string())];
        match evaluate_trust(&kh, "unknown.example", 22, &keys).unwrap_err() {
            Error::HostKeyUnknown {
                fingerprint,
                algorithm,
                key,
            } => {
                assert_eq!(algorithm, "ssh-ed25519");
                assert_eq!(key, KEY);
                assert!(fingerprint.starts_with("SHA256:"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn evaluate_trust_returns_mismatch_for_mismatched_host() {
        let dir = tempfile::tempdir().unwrap();
        let kh = dir.path().join("known_hosts");
        record(&kh, "h", 22, "ssh-ed25519", KEY).unwrap();
        let keys = [("ssh-ed25519".into(), "BBBB".to_string())];
        match evaluate_trust(&kh, "h", 22, &keys).unwrap_err() {
            Error::HostKeyMismatch {
                fingerprint,
                algorithm,
                key,
            } => {
                assert_eq!(algorithm, "ssh-ed25519");
                assert_eq!(key, "BBBB");
                assert_eq!(fingerprint, "unknown");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn evaluate_trust_returns_ok_for_known_host() {
        let dir = tempfile::tempdir().unwrap();
        let kh = dir.path().join("known_hosts");
        record(&kh, "h", 22, "ssh-ed25519", KEY).unwrap();
        let keys = [("ssh-ed25519".into(), KEY.to_string())];
        assert!(evaluate_trust(&kh, "h", 22, &keys).is_ok());
    }
}
