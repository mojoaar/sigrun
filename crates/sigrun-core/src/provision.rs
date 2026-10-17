//! Provision an SSH public key onto a remote host over a trust-gated russh session.

use base64::Engine;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use russh::client::{AuthResult, Config, Handle, Handler};
use russh::ChannelMsg;
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProvisionOutcome {
    pub installed: Vec<String>,
    pub verified: Vec<String>,
    pub hardening: Hardening,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hardening {
    NotRequested,
    Applied,
    Skipped(String),
}

pub fn authorized_keys_script(keys: &[String]) -> Result<String> {
    if keys.is_empty() {
        return Err(Error::InvalidInput("no public keys to install".into()));
    }
    let mut s = String::from(
        "set -e\numask 077\nmkdir -p \"$HOME/.ssh\"\nchmod 700 \"$HOME/.ssh\"\n\
         touch \"$HOME/.ssh/authorized_keys\"\nchmod 600 \"$HOME/.ssh/authorized_keys\"\n\
         if [ -s \"$HOME/.ssh/authorized_keys\" ] && [ -n \"$(tail -c1 \"$HOME/.ssh/authorized_keys\")\" ]; then echo >> \"$HOME/.ssh/authorized_keys\"; fi\n\
         add() { grep -qxF -- \"$1\" \"$HOME/.ssh/authorized_keys\" || printf '%s\\n' \"$1\" >> \"$HOME/.ssh/authorized_keys\"; }\n",
    );
    for k in keys {
        // Normalized to `algo b64` (validated charset), so single quotes are safe.
        s.push_str(&format!("add '{}'\n", normalize_public_key(k)?));
    }
    Ok(s)
}

const OUTPUT_CAP: usize = 8 * 1024;

fn capped_push(buf: &mut String, data: &[u8]) {
    let room = OUTPUT_CAP.saturating_sub(buf.len());
    buf.push_str(&String::from_utf8_lossy(&data[..data.len().min(room)]));
}

async fn run(handle: &mut Handle<Gate>, script: &str) -> Result<(Option<u32>, String)> {
    let mut ch = handle.channel_open_session().await?;
    ch.exec(true, "sh -s").await?;
    ch.data(script.as_bytes()).await?;
    ch.eof().await?;
    let (mut _stdout, mut stderr, mut status) = (String::new(), String::new(), None);
    while let Some(msg) = ch.wait().await {
        match msg {
            ChannelMsg::Data { data } => capped_push(&mut _stdout, &data),
            ChannelMsg::ExtendedData { data, .. } => capped_push(&mut stderr, &data),
            ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status),
            ChannelMsg::Close => break,
            _ => {}
        }
    }
    Ok((status, stderr))
}

pub const DROPIN: &str =
    "# Installed by Sigrun. Remove this file to re-enable password authentication.
PasswordAuthentication no
KbdInteractiveAuthentication no
ChallengeResponseAuthentication no
PubkeyAuthentication yes
";

pub fn hardening_script() -> String {
    format!(
        r#"D=/etc/ssh/sshd_config.d/99-sigrun.conf
C=/etc/ssh/sshd_config
command -v sudo >/dev/null || {{ echo "sudo is not available" >&2; exit 3; }}
sudo -n true 2>/dev/null || {{ echo "non-interactive sudo is required" >&2; exit 4; }}
B="$C.sigrun-bak.$(date +%s)"
sudo -n cp -p "$C" "$B" || {{ echo "could not back up sshd_config" >&2; exit 5; }}
rollback() {{ sudo -n rm -f "$D"; sudo -n cp -p "$B" "$C"; }}
dropin() {{ cat <<'SIGRUN_EOF'
{DROPIN}SIGRUN_EOF
}}
if grep -q '^Include /etc/ssh/sshd_config.d/\*.conf' "$C"; then
  dropin | sudo -n tee "$D" >/dev/null || {{ rollback; echo "could not write drop-in" >&2; exit 5; }}
else
  {{ echo '# BEGIN sigrun'; dropin; echo '# END sigrun'; }} | sudo -n tee -a "$C" >/dev/null || {{ rollback; echo "could not update sshd_config" >&2; exit 5; }}
fi
sudo -n sshd -t || {{ rollback; echo "sshd config test failed" >&2; exit 5; }}
sudo -n systemctl reload sshd || sudo -n systemctl reload ssh || sudo -n service ssh reload || {{ rollback; echo "sshd reload failed" >&2; exit 6; }}
"#
    )
}

async fn harden(handle: &mut Handle<Gate>) -> Result<Hardening> {
    let (code, stderr) = run(handle, &hardening_script()).await?;
    let reason = stderr.trim();
    match code {
        Some(0) => Ok(Hardening::Applied),
        Some(3) => Ok(Hardening::Skipped(
            if reason.is_empty() {
                "sudo is not available"
            } else {
                reason
            }
            .into(),
        )),
        Some(4) => Ok(Hardening::Skipped(
            if reason.is_empty() {
                "non-interactive sudo is required"
            } else {
                reason
            }
            .into(),
        )),
        Some(5 | 6) => Err(Error::Config(format!("sshd hardening failed: {reason}"))),
        Some(c) => Err(Error::Config(format!(
            "sshd hardening failed (exit {c}): {reason}"
        ))),
        None => Err(Error::Config(format!(
            "sshd hardening failed (exit unknown): {reason}"
        ))),
    }
}

pub async fn install_keys(
    opts: &ProvisionOptions,
    keys: &[ProvisionKey],
) -> Result<ProvisionOutcome> {
    let public_keys = keys
        .iter()
        .map(|k| normalize_public_key(&k.public_openssh))
        .collect::<Result<Vec<_>>>()?;
    let privates = keys
        .iter()
        .map(|k| {
            russh::keys::PrivateKey::from_openssh(&k.private_pem)
                .map_err(|_| Error::Key(format!("{}: invalid private key", k.name)))
        })
        .collect::<Result<Vec<_>>>()?;
    let script = authorized_keys_script(&public_keys)?;
    let mut handle = authenticate(opts).await?;
    let (code, stderr) = run(&mut handle, &script).await?;
    match code {
        Some(0) => {}
        Some(code) => {
            return Err(Error::Config(format!(
                "authorized_keys update failed (exit {code}): {}",
                stderr.trim()
            )))
        }
        None => {
            return Err(Error::Config(
                "authorized_keys update failed: no exit status from the server".into(),
            ))
        }
    }
    let _ = handle
        .disconnect(russh::Disconnect::ByApplication, "", "en")
        .await;

    let mut verified = Vec::new();
    let mut failures = Vec::new();
    for (key, private) in keys.iter().zip(privates) {
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
        let mut h = match tokio::time::timeout(Duration::from_secs(20), connect).await {
            Err(_) => {
                failures.push(format!("{}: connection timed out", key.name));
                continue;
            }
            Ok(Err(_)) => {
                failures.push(format!("{}: connection failed", key.name));
                continue;
            }
            Ok(Ok(h)) => h,
        };
        let hash = private
            .algorithm()
            .is_rsa()
            .then_some(russh::keys::HashAlg::Sha256);
        let with_hash = russh::keys::PrivateKeyWithHashAlg::new(Arc::new(private), hash);
        match h
            .authenticate_publickey(opts.user.as_str(), with_hash)
            .await
        {
            Ok(AuthResult::Success) => verified.push(key.name.clone()),
            Ok(_) => failures.push(format!("{}: authentication rejected", key.name)),
            Err(_) => failures.push(format!("{}: authentication error", key.name)),
        }
        let _ = h
            .disconnect(russh::Disconnect::ByApplication, "", "en")
            .await;
    }
    if !failures.is_empty() {
        return Err(Error::Config(format!(
            "verified: {}; failed: {}",
            verified.join(", "),
            failures.join(", ")
        )));
    }
    let hardening = if opts.disable_password_auth {
        let mut h = authenticate(opts).await?;
        let r = harden(&mut h).await;
        let _ = h
            .disconnect(russh::Disconnect::ByApplication, "", "en")
            .await;
        r?
    } else {
        Hardening::NotRequested
    };
    Ok(ProvisionOutcome {
        installed: keys.iter().map(|k| k.name.clone()).collect(),
        verified,
        hardening,
    })
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
    fn script_is_idempotent_and_quotes_safely() {
        let ed = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKKEEWQJy/IdduM5HglENeKuk+5Am8lcPpdlUdvxDWqq";
        let rsa = "ssh-rsa AAAAB3NzaC1yc2EAAAABAwAAAAEF";
        let s = authorized_keys_script(&[format!("{ed} me@x"), rsa.to_string()]).unwrap();
        assert!(s.contains("umask 077"));
        assert!(s.contains("chmod 700 \"$HOME/.ssh\""));
        assert!(s.contains("chmod 600 \"$HOME/.ssh/authorized_keys\""));
        assert!(s.contains(&format!("add '{ed}'")));
        assert!(s.contains(&format!("add '{rsa}'")));
        assert!(s.contains("grep -qxF -- \"$1\""));
        assert_eq!(s.matches("add '").count(), 2);
        assert!(s.contains("tail -c1"));
        assert!(s.contains("echo >> \"$HOME/.ssh/authorized_keys\""));
    }

    #[test]
    fn script_rejects_unsafe_keys() {
        assert!(authorized_keys_script(&["ssh-ed25519 AAAA'; rm -rf / #".to_string()]).is_err());
        assert!(authorized_keys_script(&[]).is_err());
    }

    #[test]
    fn dropin_disables_only_password_auth() {
        assert!(DROPIN.contains("PasswordAuthentication no"));
        assert!(DROPIN.contains("PubkeyAuthentication yes"));
        assert!(DROPIN.contains("KbdInteractiveAuthentication no"));
        assert!(DROPIN.contains("ChallengeResponseAuthentication no"));
    }

    #[test]
    fn hardening_script_backs_up_and_rolls_back() {
        let s = hardening_script();
        assert!(s.contains("sshd_config.d/99-sigrun.conf"));
        assert!(s.contains("cp -p"));
        assert!(s.contains("sshd -t"));
        assert!(s.contains("grep -q '^Include /etc/ssh/sshd_config.d"));
        assert!(s.contains("rm -f"));
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
