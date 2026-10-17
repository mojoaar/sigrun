//! SSH key operations.

pub mod discovery;
pub mod generate;

pub use discovery::{
    discover_keys, import_private_key, read_key, DiscoveredKey, DiscoveryResult, ImportedKey,
};
pub use generate::{generate, GeneratedKey};
