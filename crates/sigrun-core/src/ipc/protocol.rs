//! Newline-delimited JSON wire protocol for the Sigrun host IPC.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::{Connection, KeyAlgorithm, KeyView};
use crate::vault::VaultStatus;

/// The largest single IPC line, in bytes. Anything longer is rejected.
pub const MAX_LINE: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Status,
    VaultInit {
        passphrase: String,
    },
    VaultUnlock {
        passphrase: String,
    },
    VaultLock,
    ConnectionsList,
    ConnectionsAdd {
        connection: Connection,
    },
    ConnectionsUpdate {
        connection: Connection,
    },
    ConnectionsRemove {
        id: String,
    },
    ConnectionsConnect {
        id: String,
        print_only: bool,
    },
    ConnectionsProvision {
        id: String,
        password: String,
        key_ids: Vec<String>,
        disable_password_auth: bool,
    },
    KeysList,
    KeysGenerate {
        name: String,
        algorithm: KeyAlgorithm,
        passphrase: Option<String>,
    },
    KeysImport {
        path: std::path::PathBuf,
        name: String,
        passphrase: Option<String>,
    },
    KeysRemove {
        id: String,
    },
    TerminalsList,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// Adjacently tagged: internally tagged enums cannot serialize newtype variants
// that hold a sequence (`Connections(Vec<Connection>)`, `Keys(Vec<KeyView>)`).
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Status(StatusInfo),
    Connections(Vec<Connection>),
    Keys(Vec<KeyView>),
    Key(KeyView),
    Terminals(Vec<crate::terminal::TerminalInfo>),
    Command(String),
    Provisioned(crate::provision::ProvisionOutcome),
    Unlocked { warnings: Vec<String> },
    Error { code: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusInfo {
    pub version: String,
    pub vault: VaultStatus,
    pub agent_socket: Option<String>,
    pub host_addr: String,
}

/// Serialize `value` to JSON followed by a newline.
pub fn encode<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut out = serde_json::to_vec(value).map_err(|e| Error::Ipc(e.to_string()))?;
    out.push(b'\n');
    Ok(out)
}

/// Deserialize one newline-delimited JSON line. Lines over [`MAX_LINE`] bytes
/// are rejected; the trailing newline is tolerated by `serde_json`.
pub fn decode<T: serde::de::DeserializeOwned>(line: &[u8]) -> Result<T> {
    if line.len() > MAX_LINE {
        return Err(Error::Ipc("line too long".into()));
    }
    serde_json::from_slice(line).map_err(|e| Error::Ipc(e.to_string()))
}

/// Stable machine code for an error, safe to surface over IPC. Never carries a
/// secret: the underlying variants hold no key material.
pub fn error_code(err: &Error) -> &'static str {
    match err {
        Error::VaultUninitialized => "vault_uninitialized",
        Error::VaultLocked => "vault_locked",
        Error::InvalidPassphrase => "invalid_passphrase",
        Error::VaultCorrupt(_) => "vault_corrupt",
        Error::NotFound(_) => "not_found",
        Error::AlreadyExists(_) => "already_exists",
        Error::InvalidInput(_) => "invalid_input",
        Error::Config(_) => "invalid_config",
        Error::AlreadyRunning => "already_running",
        Error::Ipc(_) => "ipc",
        Error::HostKeyUnknown(_) => "host_key_unknown",
        Error::HostKeyMismatch(_) => "host_key_mismatch",
        Error::Ssh(_) => "ssh",
        _ => "internal",
    }
}
