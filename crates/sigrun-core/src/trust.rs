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

impl TrustStatus {
    pub fn is_trusted(&self) -> bool {
        *self == Self::Trusted
    }
    pub fn is_mismatch(&self) -> bool {
        *self == Self::Mismatch
    }
    pub fn needs_confirmation(&self) -> bool {
        *self == Self::Unknown
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIKKEEWQJy/IdduM5HglENeKuk+5Am8lcPpdlUdvxDWqq";

    #[test]
    fn host_key_trust_is_tofu_and_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let kh = dir.path().join("known_hosts");
        assert!(check(&kh, "h.example", 22, "ssh-ed25519", KEY)
            .unwrap()
            .needs_confirmation());
        record(&kh, "h.example", 22, "ssh-ed25519", KEY).unwrap();
        assert!(check(&kh, "h.example", 22, "ssh-ed25519", KEY)
            .unwrap()
            .is_trusted());
        assert!(check(&kh, "h.example", 22, "ssh-ed25519", "BBBB")
            .unwrap()
            .is_mismatch());
    }

    #[test]
    fn another_key_type_is_a_first_use_prompt_not_a_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let kh = dir.path().join("known_hosts");
        record(&kh, "h.example", 22, "ssh-ed25519", KEY).unwrap();
        // The same host, asked for a key type it never offered before: this is a
        // trust prompt, not evidence that the host's identity changed.
        assert!(check(&kh, "h.example", 22, "ssh-rsa", KEY)
            .unwrap()
            .needs_confirmation());
        assert!(check(&kh, "h.example", 22, "ssh-ed25519", KEY)
            .unwrap()
            .is_trusted());
        // Only a different key of a type already known is a mismatch.
        assert!(check(&kh, "h.example", 22, "ssh-ed25519", "BBBB")
            .unwrap()
            .is_mismatch());
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
}
