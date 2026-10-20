//! Domain data model for connections, keys, and the TOML files they live in.

use serde::{Deserialize, Serialize};

/// Where a connection record came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionSource {
    /// Created and edited by Sigrun; we own the lifecycle.
    #[default]
    Managed,
    /// Imported from `~/.ssh/config`; the file stays the source of truth.
    Discovered,
}

/// Where a key's private half lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeySource {
    /// Inside Sigrun's encrypted vault.
    #[default]
    Vault,
    /// Unencrypted on disk next to the public key.
    Disk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyAlgorithm {
    #[default]
    Ed25519,
    Rsa,
    Ecdsa,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Connection {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub key_id: Option<String>,
    /// The shared identity to authenticate with. Set means the connection's own
    /// `user`/`key_id`/`has_password` are unused.
    #[serde(default)]
    pub credential_id: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source: ConnectionSource,
    #[serde(default)]
    pub disable_password_auth: bool,
    /// Host-filled: whether the vault holds a password for this connection.
    /// Ignored on write (a client cannot set it; the host clears it before
    /// persisting) and serialized as a derived value on the wire and in
    /// `data.toml`.
    #[serde(default)]
    pub has_password: bool,
    /// Host-filled: whether a launched session for this connection is still
    /// running. Ignored on write (the host clears it before persisting) and
    /// serialized as a derived value on the wire and in `data.toml`.
    #[serde(default)]
    pub active: bool,
}

impl Connection {
    /// A read-only connection adopted from a discovered `~/.ssh/config` host.
    /// No Sigrun `key_id` on purpose: plain `ssh` still honors the user's own
    /// config (including `IdentityFile`).
    pub fn from_ssh_host(host: &super::ssh_config::SshHost, id: String) -> Self {
        Self {
            id,
            label: host.alias.clone(),
            host: host.host_name.clone(),
            user: host.user.clone().unwrap_or_default(),
            port: host.port.unwrap_or(22),
            key_id: None,
            credential_id: None,
            group: None,
            tags: Vec::new(),
            source: ConnectionSource::Discovered,
            disable_password_auth: false,
            has_password: false,
            active: false,
        }
    }

    /// A new managed connection pointing at `label` with SSH defaults.
    pub fn managed(id: impl Into<String>, label: impl Into<String>) -> Self {
        let (id, label) = (id.into(), label.into());
        Self {
            host: label.clone(),
            label,
            id,
            user: String::new(),
            port: 22,
            key_id: None,
            credential_id: None,
            group: None,
            tags: Vec::new(),
            source: ConnectionSource::Managed,
            disable_password_auth: false,
            has_password: false,
            active: false,
        }
    }
}

/// Non-secret key metadata stored in `data.toml`; private material never is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyMetadata {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub algorithm: KeyAlgorithm,
    #[serde(default)]
    pub public_key: String,
    #[serde(default)]
    pub fingerprint: String,
    #[serde(default)]
    pub source: KeySource,
    #[serde(default)]
    pub disk_path: Option<String>,
    #[serde(default)]
    pub has_passphrase: bool,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub created_at: String,
}

impl KeyMetadata {
    /// New metadata stamped with the current UTC time (`created_at`).
    pub fn now(
        id: impl Into<String>,
        algorithm: KeyAlgorithm,
        public_key: impl Into<String>,
        fingerprint: impl Into<String>,
        source: KeySource,
    ) -> Self {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            algorithm,
            public_key: public_key.into(),
            fingerprint: fingerprint.into(),
            source,
            disk_path: None,
            has_passphrase: false,
            group: None,
            tags: Vec::new(),
            notes: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// A key plus whether its private half is currently usable (vault unlocked,
/// disk file present).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyView {
    #[serde(flatten)]
    pub key: KeyMetadata,
    pub available: bool,
}

/// A named identity — a login name plus a key, a password, or both — that any
/// number of connections can share. Metadata only: the password lives in the
/// vault under this id, never here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Credential {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub key_id: Option<String>,
    #[serde(default)]
    pub has_password: bool,
}

/// The whole `data.toml` document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DataFile {
    pub version: u32,
    pub connections: Vec<Connection>,
    pub keys: Vec<KeyMetadata>,
    #[serde(default)]
    pub credentials: Vec<Credential>,
}

impl Default for DataFile {
    fn default() -> Self {
        Self {
            version: 1,
            connections: Vec::new(),
            keys: Vec::new(),
            credentials: Vec::new(),
        }
    }
}

/// User preferences from `settings.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub theme: String,
    pub language: String,
    pub terminal: Option<String>,
    /// Where a connect opens: "window" or "tab". Anything else is "window".
    pub open_in: String,
    pub agent_enabled: bool,
    pub start_minimized: bool,
    /// Idle minutes before the vault auto-locks; 0 disables it.
    pub auto_lock_minutes: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            theme: "nordlys".into(),
            language: "en".into(),
            terminal: None,
            open_in: "window".into(),
            agent_enabled: true,
            start_minimized: false,
            auto_lock_minutes: 30,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_to_opening_a_window() {
        assert_eq!(Settings::default().open_in, "window");
    }

    #[test]
    fn settings_without_an_open_in_key_still_load() {
        let parsed: Settings = toml::from_str("version = 1\ntheme = \"nordlys\"\n").unwrap();
        assert_eq!(parsed.open_in, "window");
    }

    #[test]
    fn connection_defaults_do_not_disable_password_auth() {
        let c = Connection::managed("id", "box");
        assert!(!c.disable_password_auth);
        let parsed: Connection =
            serde_json::from_str(r#"{"id":"i","label":"l","host":"h","user":"u","port":22}"#)
                .unwrap();
        assert!(!parsed.disable_password_auth);
    }
}
