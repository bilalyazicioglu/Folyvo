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
    #[error("this file has no share-ready version yet")]
    ShareReadyMissing,
    #[error("the share-ready version changed from {expected} to {actual}")]
    ShareReadyChanged { expected: VersionId, actual: VersionId },
    #[error("item not found")]
    ItemMissing,
    #[error("version {0} not found")]
    VersionMissing(VersionId),
    #[error("stored object {0} is missing from the vault")]
    ObjectMissing(ContentHash),
    #[error("{0} overlaps the Folyvo vault and cannot be watched")]
    RootOverlapsVault(PathBuf),
    #[error("{0} already belongs to another file in the vault")]
    PathTaken(PathBuf),
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
    fn share_ready_changed_message_tells_the_user_what_happened() {
        let e = CoreError::ShareReadyChanged {
            expected: VersionId::from_suffix("08"),
            actual: VersionId::from_suffix("09"),
        };
        let msg = e.to_string();
        assert!(msg.contains("share-ready"), "{msg}");
        assert!(msg.contains("ver_08") && msg.contains("ver_09"), "{msg}");
    }

    #[test]
    fn io_error_names_the_path() {
        let e = CoreError::io("/tmp/a.docx", std::io::Error::other("boom"));
        assert!(e.to_string().contains("/tmp/a.docx"));
    }
}
