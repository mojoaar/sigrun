//! Lock-safe TOML persistence for settings and the connection/key data file.

use std::fs::File;

use fs2::FileExt;

use crate::error::{Error, Result};
use crate::model::{Connection, DataFile, KeyMetadata, Settings};
use crate::paths::{ensure_private_dir, write_atomic, Paths};

/// Reads and writes Sigrun's TOML files. Holds only [`Paths`]: every mutation
/// re-loads the file from disk under an exclusive lock, so concurrent writers
/// (threads or processes) never overwrite each other's changes.
pub struct Store {
    paths: Paths,
}

impl Store {
    pub fn new(paths: Paths) -> Self {
        Self { paths }
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    pub fn load_settings(&self) -> Result<Settings> {
        match std::fs::read(self.paths.settings_file()) {
            Ok(bytes) => Ok(toml::from_str(&String::from_utf8_lossy(&bytes))?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        // 0o600 too: the terminal setting can leak shell paths and commands.
        self.write_toml(self.paths.settings_file(), settings, 0o600)
    }

    pub fn load_data(&self) -> Result<DataFile> {
        let bytes = match std::fs::read(self.paths.data_file()) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(DataFile::default()),
            Err(e) => return Err(e.into()),
        };
        let data: DataFile = toml::from_str(&String::from_utf8_lossy(&bytes))?;
        if data.version != 1 {
            return Err(Error::Config(format!(
                "unsupported data version {}",
                data.version
            )));
        }
        Ok(data)
    }

    pub fn save_data(&self, data: &DataFile) -> Result<()> {
        self.write_toml(self.paths.data_file(), data, 0o600)
    }

    /// Runs `f` against the on-disk data while holding an exclusive lock on
    /// `data.lock`, then saves. Dropping the file releases the lock.
    pub fn update_data<F: FnOnce(&mut DataFile) -> Result<()>>(&self, f: F) -> Result<()> {
        ensure_private_dir(&self.paths.data_dir)?;
        let lock = File::create(self.paths.lock_file())?;
        lock.lock_exclusive()?;
        let outcome = (|| {
            let mut data = self.load_data()?;
            f(&mut data)?;
            self.save_data(&data)
        })();
        drop(lock);
        outcome
    }

    pub fn upsert_connection(&self, conn: Connection) -> Result<()> {
        self.update_data(move |data| {
            match data.connections.iter_mut().find(|c| c.id == conn.id) {
                Some(existing) => *existing = conn,
                None => data.connections.push(conn),
            }
            Ok(())
        })
    }

    pub fn remove_connection(&self, id: &str) -> Result<()> {
        self.update_data(|data| {
            let before = data.connections.len();
            data.connections.retain(|c| c.id != id);
            if data.connections.len() == before {
                return Err(Error::NotFound(format!("connection {id}")));
            }
            Ok(())
        })
    }

    pub fn upsert_key(&self, key: KeyMetadata) -> Result<()> {
        self.update_data(move |data| {
            match data.keys.iter_mut().find(|k| k.id == key.id) {
                Some(existing) => *existing = key,
                None => data.keys.push(key),
            }
            Ok(())
        })
    }

    pub fn upsert_keys(&self, new_keys: &[KeyMetadata]) -> Result<()> {
        if new_keys.is_empty() {
            return Ok(());
        }
        ensure_private_dir(&self.paths.data_dir)?;
        let lock = File::create(self.paths.lock_file())?;
        lock.lock_exclusive()?;
        let outcome = (|| {
            let mut data = self.load_data()?;
            let mut changed = false;
            for key in new_keys {
                match data.keys.iter_mut().find(|k| k.id == key.id) {
                    Some(existing) => {
                        if existing != key {
                            *existing = key.clone();
                            changed = true;
                        }
                    }
                    None => {
                        data.keys.push(key.clone());
                        changed = true;
                    }
                }
            }
            if changed {
                self.save_data(&data)?;
            }
            Ok(())
        })();
        drop(lock);
        outcome
    }

    /// Absent ids are not an error: keys disappear when their disk file does.
    pub fn remove_key(&self, id: &str) -> Result<()> {
        self.update_data(|data| {
            data.keys.retain(|k| k.id != id);
            Ok(())
        })
    }

    fn write_toml(
        &self,
        path: std::path::PathBuf,
        value: &impl serde::Serialize,
        mode: u32,
    ) -> Result<()> {
        ensure_private_dir(match path.parent() {
            Some(dir) => dir,
            None => {
                return Err(Error::Config(format!(
                    "{} has no parent directory",
                    path.display()
                )))
            }
        })?;
        write_atomic(&path, toml::to_string_pretty(value)?.as_bytes(), mode)
    }
}

#[cfg(test)]
mod tests {
    use crate::error::Error;
    use crate::model::{Credential, DataFile, KeyAlgorithm};
    use crate::paths::Paths;
    use crate::store::{Connection, Store};

    #[test]
    fn upsert_replaces_and_remove_validates_and_sweeps() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(Paths::with_roots(
            dir.path().into(),
            dir.path().into(),
            dir.path().into(),
        ));
        store
            .upsert_connection(Connection::managed("a", "A"))
            .unwrap();
        let mut replaced = Connection::managed("a", "A2");
        replaced.user = "root".into();
        store.upsert_connection(replaced).unwrap();
        let data = store.load_data().unwrap();
        assert_eq!(data.connections.len(), 1);
        assert_eq!(data.connections[0].user, "root");

        store
            .upsert_connection(Connection::managed("b", "B"))
            .unwrap();
        store.remove_connection("b").unwrap();
        assert!(matches!(
            store.remove_connection("b"),
            Err(Error::NotFound(_))
        ));
        store
            .upsert_key(crate::model::KeyMetadata::now(
                "k1",
                crate::model::KeyAlgorithm::Ed25519,
                "pub",
                "SHA256:x",
                crate::model::KeySource::Vault,
            ))
            .unwrap();
        store.remove_key("k1").unwrap();
        store.remove_key("k1").unwrap(); // absent id: no-op, still Ok
        let data = store.load_data().unwrap();
        assert_eq!(data.connections.len(), 1);
        assert!(data.keys.is_empty());
    }

    #[test]
    fn credentials_and_their_connection_reference_survive_a_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(Paths::with_roots(
            dir.path().into(),
            dir.path().into(),
            dir.path().into(),
        ));
        let mut conn = Connection::managed("c1", "box");
        conn.credential_id = Some("cred1".into());
        store.upsert_connection(conn).unwrap();
        store
            .update_data(|data| {
                data.credentials.push(Credential {
                    id: "cred1".into(),
                    label: "prod".into(),
                    user: "me".into(),
                    key_id: Some("k1".into()),
                    has_password: true,
                });
                Ok(())
            })
            .unwrap();

        let data = store.load_data().unwrap();
        assert_eq!(data.credentials.len(), 1);
        assert_eq!(data.credentials[0].label, "prod");
        assert_eq!(data.connections[0].credential_id.as_deref(), Some("cred1"));
    }

    #[test]
    fn an_old_data_file_without_credentials_still_loads() {
        let parsed: DataFile =
            toml::from_str("version = 1\nkeys = []\n\n[[connections]]\nid = \"c1\"\n").unwrap();
        assert!(parsed.credentials.is_empty());
        assert_eq!(parsed.connections[0].credential_id, None);
    }

    #[test]
    fn missing_files_return_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(Paths::with_roots(
            dir.path().into(),
            dir.path().into(),
            dir.path().into(),
        ));
        assert_eq!(store.load_settings().unwrap().theme, "nordlys");
        assert!(store.load_data().unwrap().connections.is_empty());
    }

    #[test]
    fn concurrent_writers_do_not_lose_connections() {
        let dir = tempfile::tempdir().unwrap();
        let store = std::sync::Arc::new(Store::new(Paths::with_roots(
            dir.path().into(),
            dir.path().into(),
            dir.path().into(),
        )));
        let mut handles = vec![];
        for i in 0..8 {
            let store = store.clone();
            handles.push(std::thread::spawn(move || {
                store
                    .upsert_connection(Connection::managed(format!("c{i}"), format!("h{i}")))
                    .unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(store.load_data().unwrap().connections.len(), 8);
    }

    #[test]
    fn future_version_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(Paths::with_roots(
            dir.path().into(),
            dir.path().into(),
            dir.path().into(),
        ));
        std::fs::write(
            store.paths().data_file(),
            "version = 99\nconnections = []\nkeys = []\n",
        )
        .unwrap();
        assert!(matches!(store.load_data(), Err(Error::Config(_))));
    }

    #[test]
    fn upsert_keys_batches_and_skips_write_when_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(Paths::with_roots(
            dir.path().into(),
            dir.path().into(),
            dir.path().into(),
        ));
        let key1 = crate::model::KeyMetadata::now(
            "k1",
            KeyAlgorithm::Ed25519,
            "ssh-ed25519 AAAA...",
            "fp1",
            crate::model::KeySource::Disk,
        );
        let key2 = crate::model::KeyMetadata::now(
            "k2",
            KeyAlgorithm::Ed25519,
            "ssh-ed25519 BBBB...",
            "fp2",
            crate::model::KeySource::Disk,
        );

        store.upsert_keys(&[key1.clone(), key2.clone()]).unwrap();
        let loaded = store.load_data().unwrap();
        assert_eq!(loaded.keys.len(), 2);
        assert!(loaded.keys.iter().any(|k| k.id == "k1"));
        assert!(loaded.keys.iter().any(|k| k.id == "k2"));

        let mtime_before = std::fs::metadata(store.paths().data_file())
            .unwrap()
            .modified()
            .unwrap();

        // Small sleep to ensure mtime timestamp would change if written
        std::thread::sleep(std::time::Duration::from_millis(50));

        store.upsert_keys(&[key1.clone(), key2.clone()]).unwrap();
        let mtime_after = std::fs::metadata(store.paths().data_file())
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(mtime_before, mtime_after);

        let mut modified_key1 = key1.clone();
        modified_key1.name = "renamed".into();
        store.upsert_keys(&[modified_key1]).unwrap();
        let loaded_updated = store.load_data().unwrap();
        assert_eq!(
            loaded_updated
                .keys
                .iter()
                .find(|k| k.id == "k1")
                .unwrap()
                .name,
            "renamed"
        );
    }
}
