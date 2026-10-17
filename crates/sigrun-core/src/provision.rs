//! Provision an SSH public key onto a remote host over a trust-gated russh session.

use base64::Engine;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use russh::client::{AuthResult, Config, Handle, Handler};

use crate::error::{Error, Result};
use crate::trust::{check, fingerprint, TrustStatus};

pub struct ProvisionKey {
    pub name: String,
    pub public_openssh: String,
    pub private_pem: String,
}

// No Debug derive: `password` is a secret.
pub struct ProvisionOptions {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub known_hosts: PathBuf,
    pub disable_password_auth: bool,
}

pub struct Gate {
    known_hosts: PathBuf,
    host: String,
    port: u16,
}

impl Handler for Gate {
    type Error = Error;

    async fn check_server_key(
        &mut self,
        key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool> {
        if key.certificate().is_some() {
            return Err(Error::Config("host certificates are not supported".into()));
        }
        let openssh = key
            .public_key()
            .to_openssh()
            .map_err(|e| Error::Key(e.to_string()))?;
        let (algo, rest) = openssh
            .split_once(' ')
            .ok_or_else(|| Error::Key("malformed host key".into()))?;
        let b64 = rest.split_whitespace().next().unwrap_or("");
        let status = check(&self.known_hosts, &self.host, self.port, algo, b64)?;
        let detail = || {
            let fp = fingerprint(algo, b64).unwrap_or_else(|| "unknown".into());
            format!("{fp} {algo} {b64}")
        };
        match status {
            TrustStatus::Trusted => Ok(true),
            TrustStatus::Unknown => Err(Error::HostKeyUnknown(detail())),
            TrustStatus::Mismatch => Err(Error::HostKeyMismatch(detail())),
        }
    }
}

pub async fn authenticate(opts: &ProvisionOptions) -> Result<Handle<Gate>> {
    let gate = Gate {
        known_hosts: opts.known_hosts.clone(),
        host: opts.host.clone(),
        port: opts.port,
    };
    let connect = russh::client::connect(
        Arc::new(Config::default()),
        (opts.host.as_str(), opts.port),
        gate,
    );
    let mut handle = tokio::time::timeout(Duration::from_secs(20), connect)
        .await
        .map_err(|_| Error::Config("ssh connection timed out".into()))??;
    match handle
        .authenticate_password(opts.user.as_str(), opts.password.as_str())
        .await?
    {
        AuthResult::Success => Ok(handle),
        _ => Err(Error::InvalidInput("password authentication failed".into())),
    }
}

/// Reduce an OpenSSH public key line to `"<algo> <b64>"`, dropping any comment.
pub fn normalize_public_key(raw: &str) -> Result<String> {
    const ALGOS: [&str; 5] = [
        "ssh-ed25519",
        "ssh-rsa",
        "ecdsa-sha2-nistp256",
        "ecdsa-sha2-nistp384",
        "ecdsa-sha2-nistp521",
    ];
    let bad = || Error::InvalidInput("invalid public key".into());
    let mut it = raw.split_whitespace();
    let (Some(algo), Some(b64)) = (it.next(), it.next()) else {
        return Err(bad());
    };
    let body = b64.trim_end_matches('=');
    let pad = b64.len() - body.len();
    let ok = ALGOS.contains(&algo)
        && !body.is_empty()
        && pad <= 2
        && body
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'+' || c == b'/');
    // The decoded blob must start with the length-prefixed algorithm name.
    let blob = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap_or_default();
    let ok = ok
        && blob.get(4..4 + algo.len()) == Some(algo.as_bytes())
        && blob[..4] == (algo.len() as u32).to_be_bytes();
    if ok {
        Ok(format!("{algo} {b64}"))
    } else {
        Err(bad())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::protocol::error_code;

    #[test]
    fn normalize_public_key_strips_comments_and_rejects_garbage() {
        let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKKEEWQJy/IdduM5HglENeKuk+5Am8lcPpdlUdvxDWqq me@laptop";
        assert_eq!(
            normalize_public_key(key).unwrap(),
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKKEEWQJy/IdduM5HglENeKuk+5Am8lcPpdlUdvxDWqq"
        );
        assert!(normalize_public_key("").is_err());
        assert!(normalize_public_key("ssh-ed25519").is_err());
        assert!(normalize_public_key("ssh-ed25519 bad key").is_err());
        assert!(normalize_public_key("ssh-ed25519 $()").is_err());
    }

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(
            error_code(&Error::HostKeyUnknown("SHA256:x ssh-ed25519 AAAA".into())),
            "host_key_unknown"
        );
        assert_eq!(
            error_code(&Error::HostKeyMismatch("SHA256:x ssh-ed25519 AAAA".into())),
            "host_key_mismatch"
        );
    }
}
