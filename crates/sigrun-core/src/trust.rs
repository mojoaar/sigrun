//! Trust-on-first-use host-key store (Sigrun-owned `known_hosts`).
//! Line format: `host:port algo key_b64`.
use std::path::Path;

use crate::error::Result;
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

pub fn check(path: &Path, host: &str, port: u16, algo: &str, key_b64: &str) -> Result<TrustStatus> {
    let id = format!("{host}:{port}");
    let mut status = TrustStatus::Unknown;
    for line in read(path)?.lines() {
        let mut p = line.split_whitespace();
        if let (Some(h), Some(a), Some(k)) = (p.next(), p.next(), p.next()) {
            if h == id {
                if a == algo && k == key_b64 {
                    return Ok(TrustStatus::Trusted);
                }
                status = TrustStatus::Mismatch;
            }
        }
    }
    Ok(status)
}

pub fn record(path: &Path, host: &str, port: u16, algo: &str, key_b64: &str) -> Result<()> {
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

    #[test]
    fn host_key_trust_is_tofu_and_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let kh = dir.path().join("known_hosts");
        assert!(check(&kh, "h.example", 22, "ssh-ed25519", "AAAA")
            .unwrap()
            .needs_confirmation());
        record(&kh, "h.example", 22, "ssh-ed25519", "AAAA").unwrap();
        assert!(check(&kh, "h.example", 22, "ssh-ed25519", "AAAA")
            .unwrap()
            .is_trusted());
        assert!(check(&kh, "h.example", 22, "ssh-ed25519", "BBBB")
            .unwrap()
            .is_mismatch());
    }

    #[test]
    fn algorithm_switch_is_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let kh = dir.path().join("known_hosts");
        record(&kh, "h.example", 22, "ssh-ed25519", "AAAA").unwrap();
        assert!(check(&kh, "h.example", 22, "ssh-rsa", "AAAA")
            .unwrap()
            .is_mismatch());
        assert!(check(&kh, "h.example", 22, "ssh-ed25519", "AAAA")
            .unwrap()
            .is_trusted());
    }

    #[test]
    fn record_is_idempotent_and_private() {
        let dir = tempfile::tempdir().unwrap();
        let kh = dir.path().join("sub").join("known_hosts");
        record(&kh, "h", 22, "ssh-ed25519", "AAAA").unwrap();
        record(&kh, "h", 22, "ssh-ed25519", "AAAA").unwrap();
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
}
