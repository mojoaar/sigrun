//! In-memory OpenSSH-compatible SSH agent.
//!
//! Private key material lives in memory only. It must never reach a log, an
//! error message, a warning, or the disk: no type here derives `Debug` on
//! secret-bearing data, and no warning interpolates a passphrase or PEM.

use std::sync::{Arc, RwLock};

use ssh_agent_lib::agent::Session;
use ssh_agent_lib::error::AgentError;
use ssh_agent_lib::proto::signature::RSA_SHA2_512;
use ssh_agent_lib::proto::{Identity, SignRequest};

use crate::error::{Error, Result};
use crate::model::KeyMetadata;
use crate::vault::{Secret, UnlockedVault};

/// A private key held in memory for the agent. Deliberately derives no
/// traits: it holds a private key and must not be logged or copied.
pub struct AgentKey {
    pub key_id: String,
    pub private_key: ssh_key::PrivateKey,
    pub comment: String,
}

/// The set of keys an agent session serves, cheap to clone via `Arc`.
#[derive(Clone, Default)]
pub struct AgentState {
    keys: Arc<RwLock<Vec<AgentKey>>>,
}

impl AgentState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the entire key set.
    pub fn replace(&self, keys: Vec<AgentKey>) {
        *self.keys.write().expect("agent lock poisoned") = keys;
    }

    /// Remove every key.
    pub fn clear(&self) {
        self.keys.write().expect("agent lock poisoned").clear();
    }

    pub fn contains_key_id(&self, key_id: &str) -> bool {
        self.keys
            .read()
            .expect("agent lock poisoned")
            .iter()
            .any(|key| key.key_id == key_id)
    }

    pub fn len(&self) -> usize {
        self.keys.read().expect("agent lock poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.read().expect("agent lock poisoned").is_empty()
    }
}

/// A concrete [`Session`] serving the keys held in [`AgentState`].
#[derive(Clone)]
pub struct SigrunAgent {
    pub state: AgentState,
}

impl SigrunAgent {
    pub fn new(state: AgentState) -> Self {
        Self { state }
    }
}

#[ssh_agent_lib::async_trait]
impl Session for SigrunAgent {
    async fn request_identities(&mut self) -> std::result::Result<Vec<Identity>, AgentError> {
        let keys = self.state.keys.read().expect("agent lock poisoned");
        Ok(keys
            .iter()
            .map(|key| Identity {
                credential: key.private_key.public_key().key_data().clone().into(),
                comment: key.comment.clone(),
            })
            .collect())
    }

    async fn sign(
        &mut self,
        request: SignRequest,
    ) -> std::result::Result<ssh_key::Signature, AgentError> {
        let keys = self.state.keys.read().expect("agent lock poisoned");
        let key = keys
            .iter()
            .find(|key| key.private_key.public_key().key_data() == request.credential.key_data())
            .ok_or_else(|| AgentError::other(std::io::Error::other("identity not found")))?;
        sign_raw(&key.private_key, request.flags, &request.data).map_err(AgentError::other)
    }
}

/// Sign `data` with `private_key`, honouring the SSH agent protocol `flags`.
pub fn sign_raw(
    private_key: &ssh_key::PrivateKey,
    flags: u32,
    data: &[u8],
) -> Result<ssh_key::Signature> {
    use signature::Signer as _;
    use ssh_key::private::KeypairData;

    match private_key.key_data() {
        KeypairData::Ed25519(keypair) => {
            let signing_key = ed25519_dalek::SigningKey::from_bytes(keypair.private.as_ref());
            let sig = signing_key.sign(data);
            ssh_key::Signature::new(ssh_key::Algorithm::Ed25519, sig.to_bytes().to_vec())
                .map_err(|e| Error::Agent(e.to_string()))
        }
        KeypairData::Rsa(keypair) => sign_rsa(keypair, flags, data),
        KeypairData::Ecdsa(keypair) => sign_ecdsa(keypair, data),
        _ => Err(Error::Agent("unsupported key type".into())),
    }
}

fn sign_rsa(
    keypair: &ssh_key::private::RsaKeypair,
    flags: u32,
    data: &[u8],
) -> Result<ssh_key::Signature> {
    use signature::{SignatureEncoding as _, Signer as _};

    let private_key = rsa::RsaPrivateKey::from_components(
        rsa::BigUint::try_from(&keypair.public.n)
            .map_err(|_| Error::Agent("invalid rsa modulus".into()))?,
        rsa::BigUint::try_from(&keypair.public.e)
            .map_err(|_| Error::Agent("invalid rsa exponent".into()))?,
        rsa::BigUint::try_from(&keypair.private.d)
            .map_err(|_| Error::Agent("invalid rsa exponent".into()))?,
        vec![
            rsa::BigUint::try_from(&keypair.private.p)
                .map_err(|_| Error::Agent("invalid rsa prime".into()))?,
            rsa::BigUint::try_from(&keypair.private.q)
                .map_err(|_| Error::Agent("invalid rsa prime".into()))?,
        ],
    )
    .map_err(|_| Error::Agent("invalid rsa private key".into()))?;

    if flags & RSA_SHA2_512 != 0 {
        let signing_key = rsa::pkcs1v15::SigningKey::<sha2::Sha512>::new(private_key);
        let sig = signing_key
            .try_sign(data)
            .map_err(|_| Error::Agent("rsa signing failed".into()))?;
        ssh_key::Signature::new(
            ssh_key::Algorithm::Rsa {
                hash: Some(ssh_key::HashAlg::Sha512),
            },
            sig.to_vec(),
        )
        .map_err(|e| Error::Agent(e.to_string()))
    } else {
        // `flags == 0` requests the deprecated `ssh-rsa` (SHA-1) scheme. ssh-key
        // 0.6.7 cannot represent it (`HashAlg` has no `Sha1`, and `Signature::new`
        // rejects `hash: None`), so the SHA-1 default and `RSA_SHA2_256` both sign
        // with SHA-256 here.
        let signing_key = rsa::pkcs1v15::SigningKey::<sha2::Sha256>::new(private_key);
        let sig = signing_key
            .try_sign(data)
            .map_err(|_| Error::Agent("rsa signing failed".into()))?;
        ssh_key::Signature::new(
            ssh_key::Algorithm::Rsa {
                hash: Some(ssh_key::HashAlg::Sha256),
            },
            sig.to_vec(),
        )
        .map_err(|e| Error::Agent(e.to_string()))
    }
}

fn sign_ecdsa(keypair: &ssh_key::private::EcdsaKeypair, data: &[u8]) -> Result<ssh_key::Signature> {
    use signature::Signer as _;

    let private = match keypair {
        ssh_key::private::EcdsaKeypair::NistP256 { private, .. } => private,
        _ => return Err(Error::Agent("unsupported key type".into())),
    };
    let signing_key = p256::ecdsa::SigningKey::from_slice(private.as_slice())
        .map_err(|_| Error::Agent("invalid p256 private key".into()))?;
    let sig: p256::ecdsa::Signature = signing_key
        .try_sign(data)
        .map_err(|_| Error::Agent("ecdsa signing failed".into()))?;
    ssh_key::Signature::try_from(sig).map_err(|e| Error::Agent(e.to_string()))
}

/// Load every vault-held private key into agent keys.
///
/// Returns the loaded keys plus a human-readable warning per key that could
/// not be loaded. Warnings contain no secret material (no PEM, no passphrase,
/// no derived key data). `Secret::Password` entries and keys without matching
/// metadata are skipped silently.
pub fn vault_keys(vault: &UnlockedVault, metadata: &[KeyMetadata]) -> (Vec<AgentKey>, Vec<String>) {
    let mut keys = Vec::new();
    let mut warnings = Vec::new();

    for (key_id, secret) in vault.iter() {
        let Secret::PrivateKey { pem, passphrase } = secret else {
            continue;
        };
        let Some(meta) = metadata.iter().find(|meta| &meta.id == key_id) else {
            continue;
        };

        match load_key(pem, passphrase.as_deref()) {
            Ok(private_key) => keys.push(AgentKey {
                key_id: key_id.clone(),
                private_key,
                comment: meta.name.clone(),
            }),
            Err(reason) => {
                warnings.push(format!("key {:?} could not be loaded: {reason}", meta.name))
            }
        }
    }

    (keys, warnings)
}

/// Parse and (if needed) decrypt a vault-held private key.
///
/// The `Err` side is a fixed, human-readable reason with no secret material.
fn load_key(
    pem: &str,
    passphrase: Option<&str>,
) -> std::result::Result<ssh_key::PrivateKey, &'static str> {
    let key = ssh_key::PrivateKey::from_openssh(pem).map_err(|_| "invalid key")?;
    if key.is_encrypted() {
        let Some(passphrase) = passphrase else {
            return Err("passphrase required");
        };
        key.decrypt(passphrase).map_err(|_| "invalid passphrase")
    } else {
        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::generate::generate;
    use crate::model::{KeyAlgorithm, KeyMetadata, KeySource};
    use crate::vault::{Secret, Vault};
    use ssh_agent_lib::proto::SignRequest;

    #[test]
    fn identities_are_listed() {
        let k = generate(KeyAlgorithm::Ed25519, None).unwrap();
        let key = ssh_key::PrivateKey::from_openssh(&k.private_pem).unwrap();
        let state = AgentState::new();
        state.replace(vec![AgentKey {
            key_id: "k1".into(),
            private_key: key,
            comment: "k1".into(),
        }]);
        assert!(state.contains_key_id("k1"));
        assert_eq!(state.len(), 1);
    }

    #[test]
    fn ed25519_signature_verifies() {
        let k = generate(KeyAlgorithm::Ed25519, None).unwrap();
        let key = ssh_key::PrivateKey::from_openssh(&k.private_pem).unwrap();
        let sig = sign_raw(&key, 0, b"hello").unwrap();
        let public = ssh_key::PublicKey::from_openssh(&k.public_openssh).unwrap();
        let ed = match public.key_data() {
            ssh_key::public::KeyData::Ed25519(p) => p,
            _ => panic!("wrong type"),
        };
        let seed: &[u8; 32] = ed.as_ref();
        let vk = ed25519_dalek::VerifyingKey::from_bytes(seed).unwrap();
        let bytes: [u8; 64] = sig.as_bytes().try_into().unwrap();
        assert!(vk
            .verify_strict(b"hello", &ed25519_dalek::Signature::from_bytes(&bytes))
            .is_ok());
    }

    #[test]
    fn rsa_and_ecdsa_signatures_verify() {
        use signature::Verifier as _;

        for alg in [KeyAlgorithm::Rsa, KeyAlgorithm::Ecdsa] {
            let k = generate(alg, None).unwrap();
            let key = ssh_key::PrivateKey::from_openssh(&k.private_pem).unwrap();
            let sig = sign_raw(&key, 0, b"hello").unwrap();
            let public = ssh_key::PublicKey::from_openssh(&k.public_openssh).unwrap();

            match public.key_data() {
                ssh_key::public::KeyData::Rsa(p) => {
                    // flags == 0 signs RSA with SHA-256 (see `sign_rsa`).
                    let public_key = rsa::RsaPublicKey::new(
                        rsa::BigUint::try_from(&p.n).unwrap(),
                        rsa::BigUint::try_from(&p.e).unwrap(),
                    )
                    .unwrap();
                    let vk = rsa::pkcs1v15::VerifyingKey::<sha2::Sha256>::new(public_key);
                    let rsa_sig = rsa::pkcs1v15::Signature::try_from(sig.as_bytes()).unwrap();
                    vk.verify(b"hello", &rsa_sig).unwrap();
                }
                ssh_key::public::KeyData::Ecdsa(p) => {
                    let vk = p256::ecdsa::VerifyingKey::try_from(p).unwrap();
                    let ecdsa_sig = p256::ecdsa::Signature::try_from(&sig).unwrap();
                    vk.verify(b"hello", &ecdsa_sig).unwrap();
                }
                _ => panic!("unexpected key type"),
            }
        }
    }

    #[tokio::test]
    async fn request_for_an_unknown_key_errors() {
        let state = AgentState::new();
        let k = generate(KeyAlgorithm::Ed25519, None).unwrap();
        let key = ssh_key::PrivateKey::from_openssh(&k.private_pem).unwrap();
        let mut agent = SigrunAgent { state };
        let req = SignRequest {
            credential: key.public_key().key_data().clone().into(),
            data: b"x".to_vec(),
            flags: 0,
        };
        assert!(Session::sign(&mut agent, req).await.is_err());
    }

    #[test]
    fn encrypted_vault_key_is_decrypted_for_the_agent() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::new(dir.path().join("vault.json"));
        vault.create("pass").unwrap();
        let mut v = vault.unlock("pass").unwrap();
        let k = generate(KeyAlgorithm::Ed25519, Some("keypw")).unwrap();
        v.put(
            "k1",
            Secret::PrivateKey {
                pem: k.private_pem,
                passphrase: Some("keypw".into()),
            },
        )
        .unwrap();
        let meta = KeyMetadata::now(
            "k1".to_string(),
            KeyAlgorithm::Ed25519,
            k.public_openssh,
            k.fingerprint,
            KeySource::Vault,
        );
        let (keys, warnings) = vault_keys(&v, &[meta]);
        assert_eq!(keys.len(), 1);
        assert!(warnings.is_empty());
    }
}
