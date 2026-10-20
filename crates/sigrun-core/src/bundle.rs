//! Export and import of connections and public key metadata as a folder or a
//! `.zip` file. Nothing here is a secret: the vault, private keys and stored
//! passwords never take part.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::{Connection, Credential, DataFile, KeyMetadata};
use crate::terminal::safe_host_bytes;

/// The only version marker in the format.
pub const FORMAT_VERSION: u32 = 1;
/// The machine-readable part of a bundle, in both the folder and the zip.
pub const EXPORT_FILE: &str = "export.json";
/// What this bundle is, and what it is not.
pub const README_FILE: &str = "README.txt";
/// Where the human-readable public keys live.
pub const KEY_DIR: &str = "keys";

/// Reading limits for an untrusted zip: fail closed rather than exhaust memory.
const MAX_ENTRIES: usize = 512;
const MAX_ENTRY_BYTES: u64 = 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024;

/// The parsed contents of an export, exactly what `export.json` holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bundle {
    pub format: u32,
    pub exported_at: u64,
    #[serde(default)]
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub keys: Vec<KeyMetadata>,
}

/// What an export wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportSummary {
    pub path: String,
    pub connections: usize,
    pub keys: usize,
}

/// What an import changed, and what it refused to touch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportSummary {
    pub added: usize,
    pub skipped: usize,
    pub invalid: usize,
    pub unresolved_keys: usize,
}

/// Takes a copy of the data as an export: connections with the host-filled
/// flags cleared (they are not ours to write), and the key metadata verbatim.
pub fn build_bundle(data: &DataFile, now_unix: u64) -> Bundle {
    Bundle {
        format: FORMAT_VERSION,
        exported_at: now_unix,
        connections: data
            .connections
            .iter()
            .map(|conn| Connection {
                has_password: false,
                active: false,
                ..conn.clone()
            })
            .collect(),
        keys: data
            .keys
            .iter()
            .map(|key| KeyMetadata {
                // A disk path points into the exporting machine's filesystem,
                // so it is meaningless here and is not shipped.
                disk_path: None,
                ..key.clone()
            })
            .collect(),
    }
}

/// The text of `README.txt`.
pub fn readme_text() -> String {
    format!(
        "Sigrun export (format {FORMAT_VERSION})\n\
         \n\
         This is a transfer, not a backup. It holds your connections and the\n\
         public halves of your keys.\n\
         \n\
         Private keys and stored connection passwords are never written here:\n\
         they live only in the encrypted vault. To restore those, keep the vault\n\
         file and the passphrase that opens it.\n"
    )
}

/// Writes a bundle as a folder: `README.txt`, `export.json` and one public key
/// per key under `keys/`.
///
/// The files hold no secret, so ordinary writes at default permissions are
/// right here; `paths::write_atomic` and `paths::ensure_private_dir` exist for
/// private files and are deliberately not used.
pub fn write_folder(bundle: &Bundle, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(EXPORT_FILE), export_json(bundle)?)?;
    std::fs::write(dir.join(README_FILE), readme_text())?;
    let keys_dir = dir.join(KEY_DIR);
    std::fs::create_dir_all(&keys_dir)?;
    for key in &bundle.keys {
        std::fs::write(keys_dir.join(key_file_name(key)), key.public_key.as_bytes())?;
    }
    Ok(())
}

/// Writes the same entries as [`write_folder`], zipped.
pub fn write_zip(bundle: &Bundle, file: &Path) -> Result<()> {
    let mut zip = zip::ZipWriter::new(File::create(file)?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file(EXPORT_FILE, options).map_err(zip_error)?;
    zip.write_all(export_json(bundle)?.as_bytes())?;
    zip.start_file(README_FILE, options).map_err(zip_error)?;
    zip.write_all(readme_text().as_bytes())?;
    for key in &bundle.keys {
        zip.start_file(format!("{KEY_DIR}/{}", key_file_name(key)), options)
            .map_err(zip_error)?;
        zip.write_all(key.public_key.as_bytes())?;
    }
    zip.finish().map_err(zip_error)?;
    Ok(())
}

/// Reads a bundle from a folder or a `.zip` file. An unknown format is refused.
pub fn read_bundle(path: &Path) -> Result<Bundle> {
    let text = if path.is_dir() {
        std::fs::read_to_string(path.join(EXPORT_FILE))?
    } else {
        read_zip_entry(path)?
    };
    let bundle: Bundle = serde_json::from_str(&text)?;
    if bundle.format != FORMAT_VERSION {
        return Err(Error::InvalidInput(format!(
            "this export was written by a newer Sigrun (format {}); update Sigrun to import it",
            bundle.format
        )));
    }
    Ok(bundle)
}

/// Adds every bundle connection not already present, by `id`. Nothing is
/// deleted or overwritten, and the host-filled flags are forced false whatever
/// the file said.
pub fn merge(data: &mut DataFile, bundle: &Bundle) -> ImportSummary {
    let mut summary = ImportSummary::default();
    for conn in &bundle.connections {
        if !valid_connection(conn) {
            summary.invalid += 1;
            continue;
        }
        if data
            .connections
            .iter()
            .any(|existing| existing.id == conn.id)
        {
            summary.skipped += 1;
            continue;
        }
        let mut added = conn.clone();
        added.has_password = false;
        added.active = false;
        if let Some(key_id) = &added.key_id {
            if !data.keys.iter().any(|key| &key.id == key_id) {
                summary.unresolved_keys += 1;
            }
        }
        data.connections.push(added);
        summary.added += 1;
    }
    summary
}

/// Whether a connection from an untrusted export is worth storing: a non-empty
/// id and label, a host the launcher accepts, a user it accepts or none, a
/// usable port, and one identity source — a credential or the connection's own
/// user/key/password, never both.
pub fn valid_connection(conn: &Connection) -> bool {
    !conn.id.is_empty()
        && !conn.label.is_empty()
        && safe_host_bytes(&conn.host)
        && (conn.user.is_empty() || safe_host_bytes(&conn.user))
        && conn.port != 0
        && match &conn.credential_id {
            Some(_) => conn.user.is_empty() && conn.key_id.is_none() && !conn.has_password,
            None => true,
        }
}

/// Whether a credential from an untrusted caller is worth storing: a label and a
/// user to show, at least one way to authenticate, and a label no *other*
/// credential carries. `all` is the stored list, so an edit (same `id`) is not
/// its own duplicate.
pub fn credential_valid(c: &Credential, all: &[Credential]) -> bool {
    !c.label.is_empty()
        && !c.user.is_empty()
        && (c.key_id.is_some() || c.has_password)
        && !all
            .iter()
            .any(|other| other.id != c.id && other.label == c.label)
}

/// Whether an export may be written to `dest`. `~/.ssh` is read-only by rule,
/// so a path inside `ssh_dir` by *either* reading is refused. The two readings
/// catch different shapes: `..` behind a component that does not exist yet is
/// only visible lexically, while `..` after a symlink is only visible to the
/// filesystem walk — POSIX resolves `..` after the symlink, so the path lands
/// somewhere the lexical pass never sees. `create_dir_all` follows the walk, so
/// refusing on either keeps the guarantee.
pub fn destination_is_allowed(dest: &Path, ssh_dir: &Path) -> bool {
    let ssh = canonical_or_nearest(ssh_dir);
    !canonical_or_nearest(dest).starts_with(&ssh)
        && !canonical_or_nearest(&lexical_normalise(dest)).starts_with(&ssh)
}

/// Drops `.`, collapses `..` against the preceding component, and never walks
/// above a root, so `/a/../../b` reads as `/b` however little of it exists.
fn lexical_normalise(path: &Path) -> PathBuf {
    let mut parts: Vec<std::path::Component> = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => match parts.last() {
                Some(std::path::Component::Normal(_)) => {
                    parts.pop();
                }
                Some(std::path::Component::ParentDir) | None => parts.push(component),
                // A root cannot be popped, so `..` there is a no-op.
                _ => {}
            },
            other => parts.push(other),
        }
    }
    parts.iter().collect()
}

/// Serialises the bundle the way `export.json` stores it.
fn export_json(bundle: &Bundle) -> Result<String> {
    Ok(serde_json::to_string_pretty(bundle)?)
}

/// A key's name becomes a file name, so it is forced into `[A-Za-z0-9._-]` and
/// never left empty or `.`/`..`. The fingerprint disambiguates two keys that
/// share a name.
fn key_file_name(key: &KeyMetadata) -> String {
    let suffix: String = key
        .fingerprint
        .bytes()
        .filter(u8::is_ascii_alphanumeric)
        .map(char::from)
        .take(32)
        .collect();
    let suffix = if suffix.is_empty() {
        safe_file_stem(&key.id)
    } else {
        suffix
    };
    format!("{}-{suffix}.pub", safe_file_stem(&key.name))
}

/// Rewrites anything outside `[A-Za-z0-9._-]` and refuses an empty or dotted
/// stem.
fn safe_file_stem(value: &str) -> String {
    let stem: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if stem.is_empty() || stem == "." || stem == ".." {
        "key".to_string()
    } else {
        stem
    }
}

/// Reads only the `export.json` entry of a zip, within the byte limits. No
/// entry name is ever used as a path.
fn read_zip_entry(path: &Path) -> Result<String> {
    let mut archive = zip::ZipArchive::new(File::open(path)?).map_err(zip_error)?;
    if archive.len() > MAX_ENTRIES {
        return Err(Error::InvalidInput(format!(
            "zip holds more than {MAX_ENTRIES} entries"
        )));
    }
    let mut export = None;
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(zip_error)?;
        let name = entry.name().to_string();
        if !entry_name_is_safe(&name) {
            return Err(Error::InvalidInput(format!(
                "zip entry escapes the archive: {name}"
            )));
        }
        let size = entry.size();
        if size > MAX_ENTRY_BYTES {
            return Err(Error::InvalidInput(format!(
                "zip entry over {MAX_ENTRY_BYTES} bytes: {name}"
            )));
        }
        total += size;
        if total > MAX_TOTAL_BYTES {
            return Err(Error::InvalidInput(
                "zip expands past its total byte limit".into(),
            ));
        }
        if name == EXPORT_FILE {
            // Cap the read too: a declared size can lie, and `take` stops a bomb.
            let mut bytes = Vec::new();
            entry
                .by_ref()
                .take(MAX_ENTRY_BYTES + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_ENTRY_BYTES {
                return Err(Error::InvalidInput(format!(
                    "zip entry over {MAX_ENTRY_BYTES} bytes: {name}"
                )));
            }
            export = Some(
                String::from_utf8(bytes)
                    .map_err(|_| Error::InvalidInput(format!("{EXPORT_FILE} is not UTF-8")))?,
            );
        }
    }
    export.ok_or_else(|| Error::InvalidInput(format!("the bundle holds no {EXPORT_FILE}")))
}

/// Rejects an entry name that is absolute, carries a drive prefix, or contains
/// `..` or `.` components, so it could escape the archive root.
fn entry_name_is_safe(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && name.as_bytes().get(1) != Some(&b':')
        && !name.contains('\\')
        && !name.split('/').any(|part| part == ".." || part == ".")
}

/// Canonicalises `path`, or its nearest existing ancestor joined with the rest,
/// so a path that does not exist yet still compares honestly.
fn canonical_or_nearest(path: &Path) -> PathBuf {
    if let Ok(real) = path.canonicalize() {
        return real;
    }
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            canonical_or_nearest(parent).join(path.file_name().unwrap_or_default())
        }
        _ => path.to_path_buf(),
    }
}

fn zip_error(err: zip::result::ZipError) -> Error {
    Error::InvalidInput(format!("invalid zip: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Credential, KeyAlgorithm, KeySource};

    fn connection(id: &str, host: &str) -> Connection {
        let mut conn = Connection::managed(id, id);
        conn.host = host.into();
        conn.user = "me".into();
        conn
    }

    fn credential(id: &str, label: &str) -> Credential {
        Credential {
            id: id.into(),
            label: label.into(),
            user: "me".into(),
            key_id: Some("k1".into()),
            has_password: false,
        }
    }

    fn data_with_two_connections_and_one_key() -> DataFile {
        let mut second = connection("c2", "two.example");
        second.key_id = Some("k1".into());
        let mut disk_key = KeyMetadata::now(
            "k1",
            KeyAlgorithm::Ed25519,
            "ssh-ed25519 AAAA test@example",
            "SHA256:abc123",
            KeySource::Disk,
        );
        disk_key.disk_path = Some("/home/u/.ssh/id_ed25519".into());
        DataFile {
            version: 1,
            connections: vec![connection("c1", "one.example"), second],
            keys: vec![disk_key],
            credentials: vec![],
        }
    }

    #[test]
    fn a_folder_round_trips_every_connection_and_public_key() {
        let dir = tempfile::tempdir().unwrap();
        let mut data = data_with_two_connections_and_one_key();
        // An empty user is a real state — ssh then uses the local user — so the
        // round trip must carry it rather than substitute a name.
        data.connections[0].user = String::new();
        let bundle = build_bundle(&data, 1_700_000_000);
        write_folder(&bundle, dir.path()).unwrap();
        assert!(dir.path().join(EXPORT_FILE).is_file());
        assert!(dir.path().join(README_FILE).is_file());
        let back = read_bundle(dir.path()).unwrap();
        assert!(back.connections[0].user.is_empty());
        assert_eq!(back.connections, bundle.connections);
        assert_eq!(back.keys, bundle.keys);
        assert_eq!(back.format, FORMAT_VERSION);
    }

    #[test]
    fn a_zip_round_trips_the_same_bundle_as_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = build_bundle(&data_with_two_connections_and_one_key(), 1_700_000_000);
        let folder = dir.path().join("out");
        write_folder(&bundle, &folder).unwrap();
        let path = dir.path().join("out.zip");
        write_zip(&bundle, &path).unwrap();
        assert_eq!(read_bundle(&folder).unwrap(), read_bundle(&path).unwrap());
    }

    #[test]
    fn the_bundle_carries_no_password_and_no_active_flag() {
        let mut data = data_with_two_connections_and_one_key();
        data.connections[0].has_password = true;
        data.connections[0].active = true;
        let bundle = build_bundle(&data, 1_700_000_000);
        assert!(bundle
            .connections
            .iter()
            .all(|c| !c.has_password && !c.active));
    }

    #[test]
    fn an_exported_key_keeps_its_public_identity_but_no_machine_local_path() {
        let data = data_with_two_connections_and_one_key();
        assert!(
            data.keys[0].disk_path.is_some(),
            "the fixture must carry a machine-local path"
        );
        let exported = build_bundle(&data, 1_700_000_000).keys.remove(0);
        let original = &data.keys[0];
        assert_eq!(exported.name, original.name);
        assert_eq!(exported.algorithm, original.algorithm);
        assert_eq!(exported.public_key, original.public_key);
        assert_eq!(exported.fingerprint, original.fingerprint);
        assert_eq!(exported.has_passphrase, original.has_passphrase);
        assert_eq!(exported.disk_path, None);
    }

    #[test]
    fn a_destination_inside_the_ssh_directory_is_refused() {
        let ssh = Path::new("/home/u/.ssh");
        assert!(!destination_is_allowed(Path::new("/home/u/.ssh"), ssh));
        assert!(!destination_is_allowed(
            Path::new("/home/u/.ssh/export"),
            ssh
        ));
        assert!(destination_is_allowed(Path::new("/home/u/backups"), ssh));
        // A sibling whose name merely shares the prefix is not inside.
        assert!(destination_is_allowed(Path::new("/home/u/.sshrc"), ssh));
    }

    #[test]
    fn a_destination_inside_the_ssh_directory_is_refused_after_canonicalising() {
        // Symlinks and `..` must not be a way back in.
        let root = tempfile::tempdir().unwrap();
        let ssh = root.path().join(".ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        assert!(!destination_is_allowed(&ssh.join("x"), &ssh));
        assert!(!destination_is_allowed(&ssh.join("..").join(".ssh"), &ssh));
        // A `..` behind a component that does not exist yet still lands back
        // inside: `create_dir_all` would walk the `..` even though the walk
        // here cannot canonicalise the leading component.
        assert!(!destination_is_allowed(
            &root.path().join("nonexistent").join("..").join(".ssh"),
            &ssh
        ));
        assert!(destination_is_allowed(&root.path().join("backup"), &ssh));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_parent_into_the_ssh_directory_is_refused() {
        // Review Focus 1 names a symlink: it must not be a way in either.
        let root = tempfile::tempdir().unwrap();
        let ssh = root.path().join(".ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        let link = root.path().join("link-to-ssh");
        std::os::unix::fs::symlink(&ssh, &link).unwrap();
        assert!(!destination_is_allowed(&link.join("x"), &ssh));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_into_the_ssh_directory_followed_by_dotdot_is_refused() {
        // POSIX resolves `..` after the symlink, so this lands in `.ssh/evil`
        // and `create_dir_all` would write there, while a lexical reading alone
        // calls it `<root>/evil`.
        let root = tempfile::tempdir().unwrap();
        let ssh = root.path().join(".ssh");
        let sub = ssh.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&sub, &link).unwrap();
        assert!(!destination_is_allowed(&link.join("..").join("evil"), &ssh));
    }

    #[test]
    fn import_skips_a_connection_whose_id_already_exists() {
        let mut data = DataFile::default();
        data.connections.push(connection("c1", "here.example"));
        let summary = merge(
            &mut data,
            &build_bundle(&data_with_two_connections_and_one_key(), 1),
        );
        assert_eq!(summary.added, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(data.connections.len(), 2);
    }

    #[test]
    fn a_connection_has_exactly_one_identity_source() {
        // Every case starts from `connection`, whose own user is "me".
        let mut shared = connection("c1", "one.example");
        shared.user.clear();
        shared.credential_id = Some("cred1".into());

        let mut hybrid_user = connection("c2", "two.example");
        hybrid_user.credential_id = Some("cred1".into());

        let mut hybrid_key = connection("c3", "three.example");
        hybrid_key.user.clear();
        hybrid_key.key_id = Some("k1".into());
        hybrid_key.credential_id = Some("cred1".into());

        let mut hybrid_password = connection("c4", "four.example");
        hybrid_password.user.clear();
        hybrid_password.has_password = true;
        hybrid_password.credential_id = Some("cred1".into());

        let mut own_without_user = connection("c5", "five.example");
        own_without_user.user.clear(); // ssh then uses the local user

        let cases = [
            ("credential alone", shared, true),
            ("credential and own user", hybrid_user, false),
            ("credential and own key", hybrid_key, false),
            ("credential and own password", hybrid_password, false),
            ("own state, no user", own_without_user, true),
        ];
        for (name, conn, expected) in cases {
            assert_eq!(valid_connection(&conn), expected, "{name}");
        }
    }

    #[test]
    fn a_credential_needs_a_label_a_user_and_a_way_to_authenticate() {
        let good = credential("cred1", "prod");

        let mut no_label = credential("cred2", "prod");
        no_label.label.clear();
        let mut no_user = credential("cred3", "staging");
        no_user.user.clear();
        let mut no_secret = credential("cred4", "dev");
        no_secret.key_id = None;
        let mut password_only = credential("cred5", "ops");
        password_only.key_id = None;
        password_only.has_password = true;

        let stored = std::slice::from_ref(&good);
        assert!(credential_valid(&good, stored));
        assert!(!credential_valid(&no_label, stored), "no label");
        assert!(!credential_valid(&no_user, stored), "no user");
        assert!(!credential_valid(&no_secret, stored), "no secret");
        assert!(
            credential_valid(&password_only, stored),
            "a password alone is a way in"
        );
    }

    #[test]
    fn a_credential_label_is_unique_among_the_others_but_not_itself() {
        let first = credential("cred1", "prod");
        let duplicate = credential("cred2", "prod");
        assert!(!credential_valid(
            &duplicate,
            &[first.clone(), duplicate.clone()]
        ));
        // Saving the same credential again must not collide with its own row.
        let mut edited = first.clone();
        edited.user = "deployer".into();
        assert!(credential_valid(&edited, std::slice::from_ref(&first)));
    }

    #[test]
    fn import_skips_an_invalid_entry_and_counts_it() {
        let mut data = DataFile::default();
        let mut bundle = build_bundle(&data_with_two_connections_and_one_key(), 1);
        // A host the launcher would refuse.
        let mut no_host = connection("c3", "placeholder");
        no_host.host = String::new();
        bundle.connections.push(no_host);
        // An empty id is invalid too.
        bundle.connections.push(connection("", "no-id.example"));
        // A user the launcher would refuse, exactly as it refuses the host.
        let mut bad_user = connection("c4", "user.example");
        bad_user.user = "root$(id)".into();
        bundle.connections.push(bad_user);
        // Both a credential and its own identity: one source only.
        let mut hybrid = connection("c5", "hybrid.example");
        hybrid.credential_id = Some("cred1".into());
        bundle.connections.push(hybrid);
        let summary = merge(&mut data, &bundle);
        assert_eq!(summary.invalid, 4);
        assert_eq!(summary.added, 2);
        assert_eq!(summary.skipped, 0);
        assert_eq!(data.connections.len(), 2);
    }

    #[test]
    fn import_clears_the_host_filled_flags() {
        let mut data = DataFile::default();
        let mut bundle = build_bundle(&data_with_two_connections_and_one_key(), 1);
        bundle.connections[0].has_password = true;
        bundle.connections[0].active = true;
        merge(&mut data, &bundle);
        let stored = data.connections.iter().find(|c| c.id == "c1").unwrap();
        assert!(!stored.has_password && !stored.active);
    }

    #[test]
    fn import_counts_connections_whose_key_is_not_here() {
        let mut data = DataFile::default();
        data.keys.push(KeyMetadata::now(
            "k1",
            KeyAlgorithm::Ed25519,
            "ssh-ed25519 AAAA local",
            "SHA256:local",
            KeySource::Vault,
        ));
        let mut here = connection("c1", "one.example");
        here.key_id = Some("k1".into());
        let mut missing = connection("c2", "two.example");
        missing.key_id = Some("k2".into());
        let bundle = build_bundle(
            &DataFile {
                version: 1,
                connections: vec![here, missing],
                keys: vec![],
                credentials: vec![],
            },
            1,
        );
        let summary = merge(&mut data, &bundle);
        assert_eq!(summary.added, 2);
        assert_eq!(summary.unresolved_keys, 1);
    }

    #[test]
    fn key_file_names_are_safe_and_two_keys_with_one_name_do_not_collide() {
        let dir = tempfile::tempdir().unwrap();
        let key = |id: &str, name: &str, fingerprint: &str, public_key: &str| {
            let mut key = KeyMetadata::now(
                id,
                KeyAlgorithm::Ed25519,
                public_key,
                fingerprint,
                KeySource::Disk,
            );
            key.name = name.into();
            key
        };
        let data = DataFile {
            version: 1,
            connections: vec![],
            credentials: vec![],
            keys: vec![
                key("id-a", "../nasty name", "SHA256:aaa1", "ssh-ed25519 AAAA a"),
                key("id-b", "../nasty name", "SHA256:bbb2", "ssh-ed25519 BBBB b"),
                key("id-c", ".", "SHA256:ccc3", "ssh-ed25519 CCCC c"),
                key("id-d", "", "SHA256:ddd4", "ssh-ed25519 DDDD d"),
            ],
        };
        write_folder(&build_bundle(&data, 0), dir.path()).unwrap();
        let mut names: Vec<String> = std::fs::read_dir(dir.path().join(KEY_DIR))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names.len(), 4, "one file per key: {names:?}");
        for name in &names {
            assert!(
                name.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')),
                "unsafe file name {name}"
            );
            assert_ne!(name, ".");
            assert_ne!(name, "..");
            assert!(name.ends_with(".pub"), "expected a .pub file, got {name}");
        }
    }

    #[test]
    fn a_zip_entry_that_escapes_the_archive_is_rejected() {
        use std::io::Write;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("evil.zip");
        let bundle = build_bundle(&data_with_two_connections_and_one_key(), 1_700_000_000);
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        // A valid export alongside the escaping name: a reader with no escape
        // guard would find it and return `Ok`, so only the guard can give `Err`.
        zip.start_file(EXPORT_FILE, options).unwrap();
        zip.write_all(export_json(&bundle).unwrap().as_bytes())
            .unwrap();
        zip.start_file("../escape.json", options).unwrap();
        zip.write_all(b"{}").unwrap();
        zip.finish().unwrap();
        let err = read_bundle(&path).unwrap_err().to_string();
        assert!(err.contains("escapes the archive"), "message was: {err}");
    }

    #[test]
    fn a_zip_over_the_entry_count_limit_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("many.zip");
        let bundle = build_bundle(&data_with_two_connections_and_one_key(), 1_700_000_000);
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        // A valid export among the entries: without the count guard the reader
        // would find it and return `Ok`, so only the guard can give `Err`.
        zip.start_file(EXPORT_FILE, options).unwrap();
        zip.write_all(export_json(&bundle).unwrap().as_bytes())
            .unwrap();
        for i in 0..MAX_ENTRIES {
            zip.start_file(format!("entry-{i}"), options).unwrap();
        }
        zip.finish().unwrap();
        let err = read_bundle(&path).unwrap_err().to_string();
        assert!(
            err.contains(&format!("more than {MAX_ENTRIES} entries")),
            "message was: {err}"
        );
    }

    #[test]
    fn a_zip_over_the_per_entry_byte_limit_is_rejected() {
        use std::io::Write;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.zip");
        let bundle = build_bundle(&data_with_two_connections_and_one_key(), 1_700_000_000);
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        // A valid export alongside the oversized name: a reader with no
        // per-entry size guard would skip the oversized entry and return `Ok`,
        // so only the guard can give `Err`.
        zip.start_file(EXPORT_FILE, options).unwrap();
        zip.write_all(export_json(&bundle).unwrap().as_bytes())
            .unwrap();
        zip.start_file("oversized.bin", options).unwrap();
        zip.write_all(&vec![b'x'; MAX_ENTRY_BYTES as usize + 1])
            .unwrap();
        zip.finish().unwrap();
        let err = read_bundle(&path).unwrap_err().to_string();
        assert!(
            err.contains(&format!(
                "zip entry over {MAX_ENTRY_BYTES} bytes: oversized.bin"
            )),
            "message was: {err}"
        );
    }

    #[test]
    fn an_unknown_format_is_refused_with_a_newer_sigrun_message() {
        let dir = tempfile::tempdir().unwrap();
        let mut bundle = build_bundle(&data_with_two_connections_and_one_key(), 1);
        bundle.format = FORMAT_VERSION + 1;
        write_folder(&bundle, dir.path()).unwrap();
        let err = read_bundle(dir.path()).unwrap_err().to_string();
        assert!(err.contains("newer Sigrun"), "message was: {err}");
    }

    #[test]
    fn entry_names_may_not_escape_the_archive_root() {
        for name in ["../x", "a/../b", "/etc/passwd", "c:/windows", "a\\b", "."] {
            assert!(!entry_name_is_safe(name), "{name} should be refused");
        }
        for name in ["export.json", "keys/id.pub", "a/b/c.txt"] {
            assert!(entry_name_is_safe(name), "{name} should be allowed");
        }
    }
}
