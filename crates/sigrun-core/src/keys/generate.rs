use crate::error::{Error, Result};
use crate::model::KeyAlgorithm;
use ssh_key::Algorithm;
use ssh_key::EcdsaCurve;

pub struct GeneratedKey {
    pub private_pem: String,
    pub public_openssh: String,
    pub fingerprint: String,
    pub algorithm: KeyAlgorithm,
}

pub fn generate(algorithm: KeyAlgorithm, passphrase: Option<&str>) -> Result<GeneratedKey> {
    let ssh_algorithm = match algorithm {
        KeyAlgorithm::Ed25519 => Algorithm::Ed25519,
        KeyAlgorithm::Rsa => Algorithm::Rsa { hash: None },
        KeyAlgorithm::Ecdsa => Algorithm::Ecdsa {
            curve: EcdsaCurve::NistP256,
        },
    };

    let mut key = ssh_key::PrivateKey::random(&mut rand::rngs::OsRng, ssh_algorithm)
        .map_err(|e| Error::Key(e.to_string()))?;

    let passphrase = passphrase.filter(|p| !p.is_empty());
    if let Some(passphrase) = passphrase {
        // ssh-key encrypts by value: consumes/returns a new key rather than mutating.
        key = key
            .encrypt(&mut rand::rngs::OsRng, passphrase)
            .map_err(|e| Error::Key(e.to_string()))?;
    }

    let public_openssh = key
        .public_key()
        .to_openssh()
        .map_err(|e| Error::Key(e.to_string()))?;
    let fingerprint = key.public_key().fingerprint(Default::default()).to_string();
    let private_pem = key
        .to_openssh(ssh_key::LineEnding::LF)
        .map_err(|e| Error::Key(e.to_string()))?
        .to_string();

    Ok(GeneratedKey {
        private_pem,
        public_openssh,
        fingerprint,
        algorithm,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ed25519_roundtrips_through_openssh() {
        let k = generate(KeyAlgorithm::Ed25519, None).unwrap();
        let public = ssh_key::PublicKey::from_openssh(&k.public_openssh).unwrap();
        assert_eq!(
            public.fingerprint(Default::default()).to_string(),
            k.fingerprint
        );
        assert!(k.private_pem.contains("OPENSSH PRIVATE KEY"));
        assert_eq!(k.algorithm, KeyAlgorithm::Ed25519);
    }

    #[test]
    fn passphrase_encrypts_private_key() {
        let k = generate(KeyAlgorithm::Ed25519, Some("pw")).unwrap();
        let key = ssh_key::PrivateKey::from_openssh(&k.private_pem).unwrap();
        assert!(key.is_encrypted());
        assert!(key.decrypt("pw").is_ok());
        assert!(key.decrypt("wrong").is_err());
    }

    #[test]
    fn empty_passphrase_is_treated_as_none() {
        let k = generate(KeyAlgorithm::Ed25519, Some("")).unwrap();
        assert!(!ssh_key::PrivateKey::from_openssh(&k.private_pem)
            .unwrap()
            .is_encrypted());
    }

    #[test]
    fn rsa_and_ecdsa_are_supported() {
        let rsa = generate(KeyAlgorithm::Rsa, None).unwrap();
        assert!(rsa.public_openssh.starts_with("ssh-rsa "));
        let ecdsa = generate(KeyAlgorithm::Ecdsa, None).unwrap();
        assert!(ecdsa.public_openssh.starts_with("ecdsa-sha2-nistp256 "));
    }
}
