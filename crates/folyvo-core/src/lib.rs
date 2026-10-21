//! Folyvo core: versioning, storage and sharing logic. No UI or Tauri dependency.
#![allow(dead_code)] // TEMP: removed in Task 8A, once the vault facade uses every internal item

pub mod cas;
pub mod error;
pub mod model;
pub mod ports;
pub mod store;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub mod canonical;
