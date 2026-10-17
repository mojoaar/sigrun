//! Sigrun's SSH agent: an OpenSSH-compatible in-memory agent that serves
//! vault-held private keys so `ssh` can use them without any plaintext key
//! ever touching disk.

pub mod session;

pub use session::{sign_raw, vault_keys, AgentKey, AgentState, SigrunAgent};
