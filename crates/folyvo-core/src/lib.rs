//! Folyvo core: versioning, storage and sharing logic. No UI or Tauri dependency.

pub mod cas;
pub mod error;
pub mod model;
pub mod ports;
pub mod store;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub mod canonical;
pub mod ingest;
#[cfg(test)]
mod testutil;
pub mod vault;
pub use error::CoreError;
pub use model::Version;
pub use vault::{History, Vault};
