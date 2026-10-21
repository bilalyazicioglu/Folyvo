//! Folyvo core: versioning, storage and sharing logic. No UI or Tauri dependency.

pub mod error;
pub mod model;
pub mod ports;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
