//! Folyvo core: versioning, storage and sharing logic. No UI or Tauri dependency.

pub mod cas;
pub mod error;
pub mod model;
pub mod ports;
pub mod store;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub mod canonical;
pub mod ingest;
mod lock;
#[cfg(test)]
mod testutil;
pub mod vault;
pub use error::CoreError;
pub use model::Version;
pub use vault::{History, Vault};
pub mod dataless;
pub mod debounce;
pub mod engine;
pub mod events;
pub mod export;
pub mod handed;
pub mod ignore;
pub mod reconcile;
pub mod rescan;
pub mod restore;
pub mod runner;
pub mod watch;
pub mod which;
pub mod xattrs;
