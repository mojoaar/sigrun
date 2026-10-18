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
    ConnectionsSetPassword {
        id: String,
        password: Option<String>,
    },
    PasswordGet {
        id: String,
    },
    ConnectionsConnect {
        id: String,
        print_only: bool,
        // `serde(default)`: an older client's frame (no `force`) still decodes.
        #[serde(default)]
        force: bool,
    },
    ConnectionsProvision {
        id: String,
        password: String,
        key_ids: Vec<String>,
        disable_password_auth: bool,
    },
    /// A launched session ended. Sent by the shell line Sigrun launched, never
    /// by a user; an unknown id is a no-op.
    ConnectionEnded {
        id: String,
    },
    /// A launched shell's own pid, reported once before `ssh` runs. The host
    /// watches it because a terminal that kills the whole process group
    /// (closing a Ghostty tab) runs no handler inside that shell, so the end
    /// notify never comes. Sent by the launched line, never by a user; the
    /// field name rides `rename_all = "snake_case"` for free.
    SessionStarted {
        id: String,
        pid: u32,
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

#[derive(Clone, PartialEq, Serialize, Deserialize)]
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
    Password(String),
    Error { code: String, message: String },
}

// Manual `Debug` because `Response::Password` carries a secret. Every other
// variant prints exactly as the derive would; the password is redacted.
impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Response::Ok => f.write_str("Ok"),
            Response::Status(v) => f.debug_tuple("Status").field(v).finish(),
            Response::Connections(v) => f.debug_tuple("Connections").field(v).finish(),
            Response::Keys(v) => f.debug_tuple("Keys").field(v).finish(),
            Response::Key(v) => f.debug_tuple("Key").field(v).finish(),
            Response::Terminals(v) => f.debug_tuple("Terminals").field(v).finish(),
            Response::Command(v) => f.debug_tuple("Command").field(v).finish(),
            Response::Provisioned(v) => f.debug_tuple("Provisioned").field(v).finish(),
            Response::Unlocked { warnings } => f
                .debug_struct("Unlocked")
                .field("warnings", warnings)
                .finish(),
            Response::Password(_) => f.write_str("Password(\"<redacted>\")"),
            Response::Error { code, message } => f
                .debug_struct("Error")
                .field("code", code)
                .field("message", message)
                .finish(),
        }
    }
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
        Error::Unreachable(_) => "unreachable",
        Error::Ssh(_) => "ssh",
        _ => "internal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_response_debug_is_redacted() {
        let debug = format!("{:?}", Response::Password("s3cret".to_string()));
        assert!(!debug.contains("s3cret"), "{debug}");
        assert!(debug.contains("redacted"), "{debug}");
    }
}
