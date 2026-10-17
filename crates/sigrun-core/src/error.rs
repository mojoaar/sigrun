//! Typed errors for the Sigrun core.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("toml decode error: {0}")]
    TomlDecode(#[from] toml::de::Error),
    #[error("toml encode error: {0}")]
    TomlEncode(#[from] toml::ser::Error),
    #[error("invalid configuration: {0}")]
    Config(String),
    #[error("vault is not initialized")]
    VaultUninitialized,
    #[error("vault is locked")]
    VaultLocked,
    #[error("invalid passphrase")]
    InvalidPassphrase,
    #[error("vault is corrupt: {0}")]
    VaultCorrupt(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("already exists: {0}")]
    AlreadyExists(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("key error: {0}")]
    Key(String),
    #[error("agent error: {0}")]
    Agent(String),
    #[error("ipc error: {0}")]
    Ipc(String),
    #[error("another Sigrun host is already running")]
    AlreadyRunning,
    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),
    #[error("host key is not trusted: {0}")]
    HostKeyUnknown(String),
    #[error("host key does not match known_hosts: {0}")]
    HostKeyMismatch(String),
    #[error("ssh error: {0}")]
    Ssh(#[from] russh::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
