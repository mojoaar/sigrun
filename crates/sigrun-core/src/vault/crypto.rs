//! Argon2id key derivation and XChaCha20-Poly1305 authenticated encryption
//! for the vault.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chacha20poly1305::aead::{Aead, AeadCore, OsRng};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::error::{Error, Result};

/// Plaintext sealed against the vault key to produce the on-disk check value.
pub const CHECK_PLAINTEXT: &[u8] = b"sigrun-vault-v1";

/// Argon2id parameters, stored verbatim in the vault envelope so the key can be
/// re-derived from a later passphrase.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KdfParams {
    pub algorithm: String,
    pub m_cost_kib: u32,
    pub t_cost: u32,
    pub p_cost: u32,
    pub salt_b64: String,
}

impl KdfParams {
    /// Pinned Argon2id parameters for the given 16-byte salt.
    pub fn new(salt: [u8; 16]) -> KdfParams {
        KdfParams {
            algorithm: "argon2id".to_string(),
            m_cost_kib: 65_536,
            t_cost: 3,
            p_cost: 4,
            salt_b64: STANDARD.encode(salt),
        }
    }

    /// Decodes the base64 salt back to its 16 bytes.
    pub fn salt(&self) -> Result<[u8; 16]> {
        let bytes = STANDARD
            .decode(&self.salt_b64)
            .map_err(|e| Error::VaultCorrupt(format!("invalid salt: {e}")))?;
        bytes
            .try_into()
            .map_err(|_| Error::VaultCorrupt("salt is not 16 bytes".to_string()))
    }
}

/// Derives the 32-byte vault key from a passphrase under `params`.
pub fn derive_key(passphrase: &str, params: &KdfParams) -> Result<Zeroizing<[u8; 32]>> {
    if params.algorithm != "argon2id" {
        return Err(Error::VaultCorrupt(format!(
            "unsupported KDF algorithm: {}",
            params.algorithm
        )));
    }
    if !(8_192..=262_144).contains(&params.m_cost_kib) {
        return Err(Error::VaultCorrupt(format!(
            "m_cost {} KiB is outside safe range (8192..=262144)",
            params.m_cost_kib
        )));
    }
    if !(1..=10).contains(&params.t_cost) {
        return Err(Error::VaultCorrupt(format!(
            "t_cost {} is outside safe range (1..=10)",
            params.t_cost
        )));
    }
    if !(1..=16).contains(&params.p_cost) {
        return Err(Error::VaultCorrupt(format!(
            "p_cost {} is outside safe range (1..=16)",
            params.p_cost
        )));
    }

    let argon = Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(params.m_cost_kib, params.t_cost, params.p_cost, Some(32))
            .map_err(|e| Error::Config(e.to_string()))?,
    );
    let salt = params.salt()?;
    let mut key = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(passphrase.as_bytes(), &salt, key.as_mut_slice())
        .map_err(|e| Error::Config(e.to_string()))?;
    Ok(key)
}

/// An authenticated ciphertext together with the nonce that produced it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sealed {
    pub nonce: [u8; 24],
    pub ciphertext: Vec<u8>,
}

/// Encrypts `plaintext` under `key` with a fresh random nonce.
pub fn seal(key: &[u8; 32], plaintext: &[u8]) -> Result<Sealed> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| Error::Config(e.to_string()))?;
    let mut nonce_bytes = [0u8; 24];
    nonce_bytes.copy_from_slice(&nonce);
    Ok(Sealed {
        nonce: nonce_bytes,
        ciphertext,
    })
}

/// Decrypts a ciphertext produced by [`seal`]. Returns [`Error::InvalidPassphrase`]
/// on authentication failure; the vault layer distinguishes check-value failures
/// from payload failures.
pub fn open(key: &[u8; 32], nonce: &[u8; 24], ciphertext: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(XNonce::from_slice(nonce), ciphertext)
        .map_err(|_| Error::InvalidPassphrase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_are_pinned() {
        let p = KdfParams::new([7u8; 16]);
        assert_eq!((p.m_cost_kib, p.t_cost, p.p_cost), (65_536, 3, 4));
        assert_eq!(p.algorithm, "argon2id");
        assert_eq!(p.salt().unwrap(), [7u8; 16]);
    }

    #[test]
    fn seal_open_roundtrip() {
        let key = derive_key("correct horse", &KdfParams::new([1u8; 16])).unwrap();
        let sealed = seal(&key, b"secret").unwrap();
        assert_eq!(
            open(&key, &sealed.nonce, &sealed.ciphertext).unwrap(),
            b"secret"
        );
    }

    #[test]
    fn wrong_key_and_tamper_fail() {
        let key = derive_key("a", &KdfParams::new([2u8; 16])).unwrap();
        let other = derive_key("b", &KdfParams::new([2u8; 16])).unwrap();
        let sealed = seal(&key, b"secret").unwrap();
        assert!(open(&other, &sealed.nonce, &sealed.ciphertext).is_err());
        let mut ct = sealed.ciphertext.clone();
        ct[0] ^= 1;
        assert!(open(&key, &sealed.nonce, &ct).is_err());
    }

    #[test]
    fn every_seal_has_a_fresh_nonce() {
        let key = derive_key("a", &KdfParams::new([3u8; 16])).unwrap();
        assert_ne!(
            seal(&key, b"x").unwrap().nonce,
            seal(&key, b"x").unwrap().nonce
        );
    }

    #[test]
    fn excessive_kdf_params_are_rejected() {
        let mut params = KdfParams::new([0u8; 16]);
        params.m_cost_kib = 1_000_000; // 1 GiB
        let res = derive_key("pass", &params);
        assert!(matches!(res, Err(Error::VaultCorrupt(_))));

        params.m_cost_kib = 65_536;
        params.t_cost = 100;
        let res = derive_key("pass", &params);
        assert!(matches!(res, Err(Error::VaultCorrupt(_))));
    }
}
