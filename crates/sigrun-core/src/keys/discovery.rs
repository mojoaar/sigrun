//! Discovery and import of existing SSH keys from `~/.ssh`.

use crate::error::{Error, Result};
use crate::model::KeyAlgorithm;
use ssh_key::Algorithm;
use std::ffi::OsStr;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredKey {
    pub path: std::path::PathBuf,
    pub public_key: String,
    pub fingerprint: String,
    pub algorithm: KeyAlgorithm,
    pub has_passphrase: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveryResult {
    pub keys: Vec<DiscoveredKey>,
    pub warnings: Vec<String>,
}

pub struct ImportedKey {
    pub private_pem: String,
    pub public_key: String,
    pub fingerprint: String,
    pub algorithm: KeyAlgorithm,
}

fn map_algorithm(algorithm: Algorithm) -> Option<KeyAlgorithm> {
    match algorithm {
        Algorithm::Ed25519 => Some(KeyAlgorithm::Ed25519),
        Algorithm::Rsa { .. } => Some(KeyAlgorithm::Rsa),
        Algorithm::Ecdsa { .. } => Some(KeyAlgorithm::Ecdsa),
        _ => None,
    }
}

fn is_skipped(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    matches!(
        name.as_ref(),
        "config"
            | "known_hosts"
            | "known_hosts.old"
            | "authorized_keys"
            | "authorized_keys2"
            | "environment"
            | "rc"
    ) || name.ends_with(".pub")
}

/// Names not present in this directory listing are skipped without a warning:
/// they may be unrelated files (`known_hosts`, `notes.txt`, ...).
fn discover_one(path: &Path, warnings: &mut Vec<String>) -> Result<Option<DiscoveredKey>> {
    let key = match ssh_key::PrivateKey::read_openssh_file(path) {
        Ok(key) => key,
        Err(_) => return Ok(None),
    };

    let algorithm = match map_algorithm(key.algorithm()) {
        Some(algorithm) => algorithm,
        None => {
            warnings.push(format!("unsupported key algorithm at {}", path.display()));
            return Ok(None);
        }
    };

    if key.is_encrypted() {
        let companion = match std::fs::read_to_string(path.with_extension("pub")) {
            Ok(text) => match ssh_key::PublicKey::from_openssh(&text) {
                Ok(public) => public,
                Err(_) => return Ok(None),
            },
            Err(_) => {
                warnings.push(format!(
                    "encrypted key has no .pub companion at {}",
                    path.display()
                ));
                return Ok(None);
            }
        };
        return Ok(Some(DiscoveredKey {
            path: path.to_path_buf(),
            public_key: companion
                .to_openssh()
                .map_err(|e| Error::Key(e.to_string()))?,
            fingerprint: companion.fingerprint(Default::default()).to_string(),
            algorithm,
            has_passphrase: true,
        }));
    }

    let public = key.public_key();
    Ok(Some(DiscoveredKey {
        path: path.to_path_buf(),
        public_key: public.to_openssh().map_err(|e| Error::Key(e.to_string()))?,
        fingerprint: public.fingerprint(Default::default()).to_string(),
        algorithm,
        has_passphrase: false,
    }))
}

pub fn discover_keys(dir: &Path) -> Result<DiscoveryResult> {
    let mut warnings = Vec::new();
    if !dir.exists() {
        return Ok(DiscoveryResult {
            keys: Vec::new(),
            warnings,
        });
    }

    let entries = std::fs::read_dir(dir).map_err(Error::Io)?;
    let mut keys = Vec::new();
    for entry in entries {
        let path = entry.map_err(Error::Io)?.path();
        if path.is_dir() {
            continue;
        }
        match path.file_name() {
            Some(name) if is_skipped(name) => continue,
            Some(_) => {}
            None => continue,
        }
        if let Some(key) = discover_one(&path, &mut warnings)? {
            keys.push(key);
        }
    }

    keys.sort_by(|a, b| a.path.cmp(&b.path));
    let mut seen = std::collections::HashSet::new();
    keys.retain(|k| seen.insert(k.fingerprint.clone()));

    Ok(DiscoveryResult { keys, warnings })
}

pub fn read_key(path: &Path) -> Result<DiscoveredKey> {
    let key = ssh_key::PrivateKey::read_openssh_file(path)
        .map_err(|_| Error::Key(format!("cannot read private key at {}", path.display())))?;

    let algorithm = map_algorithm(key.algorithm())
        .ok_or_else(|| Error::Key(format!("unsupported key algorithm at {}", path.display())))?;

    if key.is_encrypted() {
        let text = std::fs::read_to_string(path.with_extension("pub")).map_err(|_| {
            Error::Key(
                "passphrase-protected key has no .pub companion; import it with a passphrase"
                    .to_string(),
            )
        })?;
        let companion = ssh_key::PublicKey::from_openssh(&text).map_err(|_| {
            Error::Key(format!("cannot parse companion .pub at {}", path.display()))
        })?;
        return Ok(DiscoveredKey {
            path: path.to_path_buf(),
            public_key: companion
                .to_openssh()
                .map_err(|e| Error::Key(e.to_string()))?,
            fingerprint: companion.fingerprint(Default::default()).to_string(),
            algorithm,
            has_passphrase: true,
        });
    }

    let public = key.public_key();
    Ok(DiscoveredKey {
        path: path.to_path_buf(),
        public_key: public.to_openssh().map_err(|e| Error::Key(e.to_string()))?,
        fingerprint: public.fingerprint(Default::default()).to_string(),
        algorithm,
        has_passphrase: false,
    })
}

pub fn import_private_key(path: &Path, passphrase: Option<&str>) -> Result<ImportedKey> {
    let key = ssh_key::PrivateKey::read_openssh_file(path)
        .map_err(|_| Error::Key(format!("cannot read private key at {}", path.display())))?;

    let algorithm = map_algorithm(key.algorithm())
        .ok_or_else(|| Error::Key(format!("unsupported key algorithm at {}", path.display())))?;

    let key = if key.is_encrypted() {
        let passphrase = passphrase.filter(|p| !p.is_empty());
        let Some(passphrase) = passphrase else {
            return Err(Error::InvalidInput(
                "key is encrypted; a passphrase is required".to_string(),
            ));
        };
        key.decrypt(passphrase)
            .map_err(|_| Error::InvalidPassphrase)?
    } else {
        // Unencrypted: the passphrase argument is ignored entirely.
        key
    };

    let public = key.public_key();
    let fingerprint = public.fingerprint(Default::default()).to_string();

    // Cross-check the .pub companion if one exists; its absence is fine.
    if let Ok(text) = std::fs::read_to_string(path.with_extension("pub")) {
        if let Ok(companion) = ssh_key::PublicKey::from_openssh(&text) {
            if companion.fingerprint(Default::default()).to_string() != fingerprint {
                return Err(Error::Key(
                    "public key does not match private key".to_string(),
                ));
            }
        }
    }

    Ok(ImportedKey {
        private_pem: key
            .to_openssh(ssh_key::LineEnding::LF)
            .map_err(|e| Error::Key(e.to_string()))?
            .to_string(),
        public_key: public.to_openssh().map_err(|e| Error::Key(e.to_string()))?,
        fingerprint,
        algorithm,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::keys::generate;
    use std::path::PathBuf;

    fn write_key(dir: &std::path::Path, name: &str, passphrase: Option<&str>) -> PathBuf {
        let k = generate(KeyAlgorithm::Ed25519, passphrase).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, k.private_pem).unwrap();
        std::fs::write(dir.join(format!("{name}.pub")), &k.public_openssh).unwrap();
        path
    }

    #[test]
    fn discovers_plain_and_encrypted_keys() {
        let dir = tempfile::tempdir().unwrap();
        write_key(dir.path(), "id_ed25519", None);
        write_key(dir.path(), "id_work", Some("pw"));
        std::fs::write(dir.path().join("known_hosts"), "junk").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "junk").unwrap();
        let found = discover_keys(dir.path()).unwrap();
        assert_eq!(found.keys.len(), 2);
        assert!(found.keys.iter().any(|k| k.has_passphrase));
    }

    #[test]
    fn encrypted_key_without_pub_is_a_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        let k = generate(KeyAlgorithm::Ed25519, Some("pw")).unwrap();
        let path = dir.path().join("id_secret");
        std::fs::write(&path, k.private_pem).unwrap();
        assert!(matches!(read_key(&path), Err(Error::Key(_))));
    }

    #[test]
    fn wrong_import_passphrase_is_invalid_passphrase() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_key(dir.path(), "id_locked", Some("right"));
        assert!(matches!(
            import_private_key(&path, Some("wrong")),
            Err(Error::InvalidPassphrase)
        ));
        assert!(import_private_key(&path, Some("right")).is_ok());
        assert!(matches!(
            import_private_key(&path, None),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            import_private_key(&path, Some("")),
            Err(Error::InvalidInput(_))
        ));
    }

    #[test]
    fn dedupes_by_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_key(dir.path(), "id_ed25519", None);
        std::fs::copy(&path, dir.path().join("copy")).unwrap();
        std::fs::copy(
            dir.path().join("id_ed25519.pub"),
            dir.path().join("copy.pub"),
        )
        .unwrap();
        assert_eq!(discover_keys(dir.path()).unwrap().keys.len(), 1);
    }

    #[test]
    fn dedupes_by_fingerprint_non_adjacent() {
        let dir = tempfile::tempdir().unwrap();
        let a = write_key(dir.path(), "a_key", None);
        write_key(dir.path(), "b_key", None);
        std::fs::copy(&a, dir.path().join("c_key")).unwrap();
        std::fs::copy(dir.path().join("a_key.pub"), dir.path().join("c_key.pub")).unwrap();
        let found = discover_keys(dir.path()).unwrap();
        assert_eq!(found.keys.len(), 2);
    }

    #[test]
    fn encrypted_key_without_pub_skips_with_warning() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_key(dir.path(), "id_extra", Some("pw"));
        std::fs::remove_file(dir.path().join("id_extra.pub")).unwrap();
        let found = discover_keys(dir.path()).unwrap();
        assert!(found.keys.iter().all(|k| k.path != path));
        assert_eq!(found.warnings.len(), 1);
        assert!(found.warnings[0].contains("id_extra"));
    }
}
