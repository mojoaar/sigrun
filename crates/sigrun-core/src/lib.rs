pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod agent;
pub mod error;
pub mod host;
pub mod ipc;
pub mod keys;
pub mod model;
pub mod paths;
pub mod ssh_config;
pub mod store;
pub mod terminal;
pub mod trust;
pub mod vault;
