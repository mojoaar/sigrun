//! Newline-delimited JSON wire protocol for the Sigrun host IPC.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::{Connection, Credential, KeyAlgorithm, KeyView, Settings};
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
    VaultChangePassphrase {
        current_passphrase: String,
        new_passphrase: String,
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
    /// Create a credential. The host assigns the id; the optional password is
    /// sealed in the same transaction, so the row never claims a secret the
    /// vault does not hold. Rotating one later is `CredentialsSetPassword`.
    CredentialsAdd {
        credential: Credential,
        password: Option<String>,
    },
    /// Replace a credential's metadata, keeping its id and its stored password.
    CredentialsUpdate {
        credential: Credential,
    },
    /// Delete an unused credential and its secret. Refused while a connection
    /// still references it.
    CredentialsRemove {
        id: String,
    },
    CredentialsSetPassword {
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
    CredentialsList,
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
    /// Write a vault key's private half to `path`, mode `0600`. Refused inside
    /// `~/.ssh` and when the file already exists.
    KeysExport {
        id: String,
        path: String,
    },
    TerminalsList,
    /// Write every connection and public key metadata to `path`, as a folder or
    /// a `.zip`. Reads only `data.toml`: no vault, no secret.
    ExportBundle {
        path: String,
        zipped: bool,
    },
    /// Merge the bundle at `path` (a folder or a `.zip`) into the store,
    /// skipping the ids already there. Never touches the vault.
    ImportBundle {
        path: String,
    },
    #[serde(rename = "settings_get")]
    SettingsGet,
    #[serde(rename = "settings_set")]
    SettingsSet {
        settings: Box<Settings>,
    },
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
    Credentials(Vec<Credential>),
    Key(KeyView),
    Terminals(Vec<crate::terminal::TerminalInfo>),
    Command(String),
    Provisioned(crate::provision::ProvisionOutcome),
    Export(crate::bundle::ExportSummary),
    Import(crate::bundle::ImportSummary),
    #[serde(rename = "settings")]
    Settings(Settings),
    Unlocked {
        warnings: Vec<String>,
    },
    Password(String),
    Error {
        code: String,
        message: String,
        /// Structured payload for errors that carry one (e.g. trust errors).
        /// Older clients ignore it; newer ones use it instead of parsing `message`.
        payload: Option<serde_json::Value>,
    },
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
            Response::Credentials(v) => f.debug_tuple("Credentials").field(v).finish(),
            Response::Key(v) => f.debug_tuple("Key").field(v).finish(),
            Response::Terminals(v) => f.debug_tuple("Terminals").field(v).finish(),
            Response::Command(v) => f.debug_tuple("Command").field(v).finish(),
            Response::Provisioned(v) => f.debug_tuple("Provisioned").field(v).finish(),
            Response::Export(v) => f.debug_tuple("Export").field(v).finish(),
            Response::Import(v) => f.debug_tuple("Import").field(v).finish(),
            Response::Settings(v) => f.debug_tuple("Settings").field(v).finish(),
            Response::Unlocked { warnings } => f
                .debug_struct("Unlocked")
                .field("warnings", warnings)
                .finish(),
            Response::Password(_) => f.write_str("Password(\"<redacted>\")"),
            Response::Error {
                code,
                message,
                payload,
            } => {
                let mut d = f.debug_struct("Error");
                d.field("code", code).field("message", message);
                if payload.is_some() {
                    d.field("payload", payload);
                }
                d.finish()
            }
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

impl From<Error> for Response {
    fn from(err: Error) -> Self {
        let code = error_code(&err).to_string();
        let message = err.to_string();
        let payload = match &err {
            Error::HostKeyUnknown {
                fingerprint,
                algorithm,
                key,
            }
            | Error::HostKeyMismatch {
                fingerprint,
                algorithm,
                key,
            } => Some(serde_json::json!({
                "fingerprint": fingerprint,
                "algorithm": algorithm,
                "key": key,
            })),
            _ => None,
        };
        Response::Error {
            code,
            message,
            payload,
        }
    }
}

/// Stable machine code for an error, safe to surface over IPC. Never carries a
/// secret: the underlying variants hold no key material.
pub fn error_code(err: &Error) -> &'static str {
    match err {
        Error::VaultUninitialized => "vault_uninitialized",
        Error::VaultLocked | Error::VaultLockedFor(_) => "vault_locked",
        Error::InvalidPassphrase => "invalid_passphrase",
        Error::VaultCorrupt(_) => "vault_corrupt",
        Error::NotFound(_) => "not_found",
        Error::AlreadyExists(_) => "already_exists",
        Error::InvalidInput(_) => "invalid_input",
        Error::Config(_) => "invalid_config",
        Error::AlreadyRunning => "already_running",
        Error::Ipc(_) => "ipc",
        Error::HostKeyUnknown { .. } => "host_key_unknown",
        Error::HostKeyMismatch { .. } => "host_key_mismatch",
        Error::Unreachable(_) => "unreachable",
        Error::Ssh(_) => "ssh",
        _ => "internal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_change_passphrase_roundtrips_json() {
        let req = Request::VaultChangePassphrase {
            current_passphrase: "old".into(),
            new_passphrase: "new".into(),
        };
        let encoded = encode(&req).unwrap();
        let decoded: Request = decode(&encoded).unwrap();
        assert_eq!(decoded, req);
    }

    #[test]
    fn password_response_debug_is_redacted() {
        let debug = format!("{:?}", Response::Password("s3cret".to_string()));
        assert!(!debug.contains("s3cret"), "{debug}");
        assert!(debug.contains("redacted"), "{debug}");
    }

    #[test]
    fn error_response_from_host_key_unknown_carries_payload() {
        let err = Error::HostKeyUnknown {
            fingerprint: "SHA256:abc".into(),
            algorithm: "ssh-ed25519".into(),
            key: "AAAA".into(),
        };
        let resp: Response = err.into();
        match resp {
            Response::Error {
                code,
                message,
                payload,
            } => {
                assert_eq!(code, "host_key_unknown");
                assert_eq!(message, "SHA256:abc ssh-ed25519 AAAA");
                let p = payload.unwrap();
                assert_eq!(p["fingerprint"], "SHA256:abc");
                assert_eq!(p["algorithm"], "ssh-ed25519");
                assert_eq!(p["key"], "AAAA");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn error_response_from_host_key_mismatch_carries_payload() {
        let err = Error::HostKeyMismatch {
            fingerprint: "SHA256:def".into(),
            algorithm: "ssh-rsa".into(),
            key: "BBBB".into(),
        };
        let resp: Response = err.into();
        match resp {
            Response::Error {
                code,
                message,
                payload,
            } => {
                assert_eq!(code, "host_key_mismatch");
                assert_eq!(message, "SHA256:def ssh-rsa BBBB");
                let p = payload.unwrap();
                assert_eq!(p["fingerprint"], "SHA256:def");
                assert_eq!(p["algorithm"], "ssh-rsa");
                assert_eq!(p["key"], "BBBB");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn error_response_from_plain_error_has_no_payload() {
        let err = Error::NotFound("x".into());
        let resp: Response = err.into();
        match resp {
            Response::Error { payload, .. } => assert!(payload.is_none()),
            other => panic!("unexpected {other:?}"),
        }
    }
}
