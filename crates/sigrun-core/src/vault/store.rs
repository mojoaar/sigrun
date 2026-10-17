//! Encrypted vault lifecycle: the on-disk envelope, create/unlock, and
//! in-memory secret storage. The vault is the only place Sigrun keeps secrets;
//! `~/.ssh` is never written.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chacha20poly1305::aead::{rand_core::RngCore, OsRng};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::error::{Error, Result};
use crate::paths::{ensure_private_dir, write_atomic};
use crate::vault::crypto::{derive_key, open, seal, KdfParams, CHECK_PLAINTEXT};

/// A single stored secret. Never derives `Debug` or `Clone`: the manual `Debug`
/// redacts every field, and cloning secrets is a hygiene smell.
#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub enum Secret {
    PrivateKey {
        pem: String,
        passphrase: Option<String>,
    },
    Password {
        password: String,
    },
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Secret::PrivateKey { .. } => f.write_str("PrivateKey { .. }"),
            Secret::Password { .. } => f.write_str("Password { .. }"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultStatus {
    Uninitialized,
    Locked,
    Unlocked,
}

/// A vault handle tied to an on-disk envelope. Cheap to construct; holds only
/// the path. Secrets live in [`UnlockedVault`] after a successful [`Vault::unlock`].
pub struct Vault {
    path: PathBuf,
}

impl Vault {
    pub fn new(path: PathBuf) -> Vault {
        Vault { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    pub fn status(&self) -> VaultStatus {
        if self.path.exists() {
            VaultStatus::Locked
        } else {
            VaultStatus::Uninitialized
        }
    }

    pub fn create(&self, passphrase: &str) -> Result<()> {
        if self.path.exists() {
            return Err(Error::AlreadyExists(self.path.display().to_string()));
        }

        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);
        let kdf = KdfParams::new(salt);
        let key = derive_key(passphrase, &kdf)?;

        let check = seal(&key, CHECK_PLAINTEXT)?;
        let payload = BTreeMap::<String, Secret>::new();
        let sealed = seal(&key, &serde_json::to_vec(&payload)?)?;

        let envelope = Envelope {
            version: 1,
            kdf,
            check_nonce: STANDARD.encode(check.nonce),
            check: STANDARD.encode(&check.ciphertext),
            nonce: STANDARD.encode(sealed.nonce),
            ciphertext: STANDARD.encode(&sealed.ciphertext),
        };
        write_envelope(&self.path, &envelope)
    }

    pub fn unlock(&self, passphrase: &str) -> Result<UnlockedVault> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::VaultUninitialized)
            }
            Err(e) => return Err(e.into()),
        };

        let envelope: Envelope = serde_json::from_slice(&bytes)?;

        if envelope.version != 1 {
            return Err(Error::VaultCorrupt(format!(
                "unsupported vault version {}",
                envelope.version
            )));
        }

        let key = derive_key(passphrase, &envelope.kdf)?;

        // A wrong passphrase derives the wrong key, so the check value fails
        // authentication; that is the only valid reason this `open` can fail.
        let check_nonce = decode_nonce(&envelope.check_nonce)?;
        let check = decode_bytes(&envelope.check)?;
        open(&key, &check_nonce, &check).map_err(|_| Error::InvalidPassphrase)?;

        // The check authenticated, so any payload failure means corruption.
        let nonce = decode_nonce(&envelope.nonce)?;
        let ciphertext = decode_bytes(&envelope.ciphertext)?;
        let plaintext = Zeroizing::new(
            open(&key, &nonce, &ciphertext)
                .map_err(|_| Error::VaultCorrupt("payload authentication failed".to_string()))?,
        );
        let payload: BTreeMap<String, Secret> =
            serde_json::from_slice(&plaintext).map_err(|e| Error::VaultCorrupt(e.to_string()))?;

        Ok(UnlockedVault {
            path: self.path.clone(),
            key,
            kdf: envelope.kdf,
            payload,
        })
    }
}

/// An unlocked vault: the derived key and decrypted payload held in memory.
/// [`UnlockedVault::put`] and [`UnlockedVault::remove`] re-seal and rewrite the
/// envelope immediately; [`UnlockedVault::lock`] consumes the handle so the key
/// is dropped (and zeroized) and the in-memory payload released.
pub struct UnlockedVault {
    path: PathBuf,
    key: Zeroizing<[u8; 32]>,
    kdf: KdfParams,
    payload: BTreeMap<String, Secret>,
}

impl UnlockedVault {
    pub fn get(&self, key: &str) -> Option<&Secret> {
        self.payload.get(key)
    }

    pub fn put(&mut self, key: &str, secret: Secret) -> Result<()> {
        self.payload.insert(key.to_string(), secret);
        self.persist()
    }

    pub fn remove(&mut self, key: &str) -> Result<()> {
        if self.payload.remove(key).is_some() {
            self.persist()?;
        }
        Ok(())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Secret)> {
        self.payload.iter()
    }

    pub fn lock(self) -> Vault {
        Vault::new(self.path)
    }

    fn persist(&self) -> Result<()> {
        let check = seal(&self.key, CHECK_PLAINTEXT)?;
        let plaintext = serde_json::to_vec(&self.payload)?;
        let sealed = seal(&self.key, &plaintext)?;
        let envelope = Envelope {
            version: 1,
            kdf: self.kdf.clone(),
            check_nonce: STANDARD.encode(check.nonce),
            check: STANDARD.encode(&check.ciphertext),
            nonce: STANDARD.encode(sealed.nonce),
            ciphertext: STANDARD.encode(&sealed.ciphertext),
        };
        write_envelope(&self.path, &envelope)
    }
}

/// On-disk JSON envelope. The secret-bearing fields are base64 STANDARD (the
/// reason [`crate::vault::crypto::Sealed`]'s array-serialized nonce never
/// appears on disk).
#[derive(Serialize, Deserialize)]
struct Envelope {
    version: u32,
    kdf: KdfParams,
    check_nonce: String,
    check: String,
    nonce: String,
    ciphertext: String,
}

fn write_envelope(path: &Path, envelope: &Envelope) -> Result<()> {
    let json = serde_json::to_vec_pretty(envelope)?;
    if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        ensure_private_dir(dir)?;
    }
    write_atomic(path, &json, 0o600)
}

fn decode_nonce(b64: &str) -> Result<[u8; 24]> {
    let bytes = STANDARD
        .decode(b64)
        .map_err(|e| Error::VaultCorrupt(format!("invalid nonce: {e}")))?;
    bytes
        .try_into()
        .map_err(|_| Error::VaultCorrupt("nonce is not 24 bytes".to_string()))
}

fn decode_bytes(b64: &str) -> Result<Vec<u8>> {
    STANDARD
        .decode(b64)
        .map_err(|e| Error::VaultCorrupt(format!("invalid base64: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    #[test]
    fn create_unlock_put_and_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::new(dir.path().join("vault.json"));
        vault.create("pass").unwrap();
        assert_eq!(vault.status(), VaultStatus::Locked);
        let mut v = vault.unlock("pass").unwrap();
        v.put(
            "k1",
            Secret::Password {
                password: "p".into(),
            },
        )
        .unwrap();
        drop(v);
        let v = vault.unlock("pass").unwrap();
        assert!(matches!(v.get("k1"), Some(Secret::Password { password }) if password == "p"));
    }

    #[test]
    fn wrong_passphrase_is_distinct_from_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::new(dir.path().join("vault.json"));
        vault.create("pass").unwrap();
        assert!(matches!(
            vault.unlock("nope"),
            Err(Error::InvalidPassphrase)
        ));

        let mut envelope: serde_json::Value =
            serde_json::from_slice(&std::fs::read(vault.path()).unwrap()).unwrap();
        envelope["ciphertext"] = "AAAA".into();
        std::fs::write(vault.path(), serde_json::to_vec(&envelope).unwrap()).unwrap();
        assert!(matches!(vault.unlock("pass"), Err(Error::VaultCorrupt(_))));
    }

    #[test]
    fn debug_does_not_leak_secrets() {
        let s = Secret::Password {
            password: "hunter2".into(),
        };
        assert!(!format!("{s:?}").contains("hunter2"));
    }

    #[cfg(unix)]
    #[test]
    fn vault_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::new(dir.path().join("vault.json"));
        vault.create("pass").unwrap();
        assert_eq!(
            std::fs::metadata(vault.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
