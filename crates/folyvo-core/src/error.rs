use std::path::PathBuf;

use crate::model::{ContentHash, VersionId};

pub type Result<T> = std::result::Result<T, CoreError>;

/// Typed, user-facing errors. The UI maps each variant to "what happened, what to do".
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("could not access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{0} changed while it was being saved to the vault; try again")]
    Unstable(PathBuf),
    #[error("file is too large for the demo ({size} bytes, limit {limit})")]
    TooLarge { size: u64, limit: u64 },
    #[error("{0} is a folder or package, not a single file")]
    NotAFile(PathBuf),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("item not found")]
    ItemMissing,
    #[error("unknown share service {0:?}")]
    BadService(String),
    #[error("this file has no version v{0}")]
    NoSuchVersionNumber(i64),
    #[error("version {0} not found")]
    VersionMissing(VersionId),
    #[error("stored object {0} is missing from the vault")]
    ObjectMissing(ContentHash),
    #[error("{0} overlaps the Folyvo vault and cannot be watched")]
    RootOverlapsVault(PathBuf),
    #[error("{0} already belongs to another file in the vault")]
    PathTaken(PathBuf),
    #[error("could not watch the folder: {0}")]
    Watch(String),
    #[error("vault data is inconsistent: {0}")]
    Corrupt(String),
}

impl CoreError {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        CoreError::Io { path: path.into(), source }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_error_names_the_path() {
        let e = CoreError::io("/tmp/a.docx", std::io::Error::other("boom"));
        assert!(e.to_string().contains("/tmp/a.docx"));
    }
}
