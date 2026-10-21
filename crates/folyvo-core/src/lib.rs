//! Folyvo core: versioning, storage and sharing logic. No UI or Tauri dependency.

pub mod model;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
