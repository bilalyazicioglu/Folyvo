use std::fmt;
use std::io::ErrorKind;
use std::path::Path;

use folyvo_core::model::IdError;
use folyvo_core::CoreError;
use serde::Serialize;
use ts_rs::TS;

/// What a person sees when something goes wrong: what happened, and what to do about it.
#[derive(Serialize, TS, Debug, Clone, PartialEq, Eq)]
#[ts(export)]
pub struct UserError {
    pub code: String,
    pub title: String,
    pub hint: String,
}

impl UserError {
    pub fn new(code: &str, title: &str, hint: &str) -> Self {
        Self { code: code.into(), title: title.into(), hint: hint.into() }
    }

    pub fn internal() -> Self {
        Self::new("internal", "Something went wrong inside Folyvo", "Nothing was changed. Try again.")
    }

    pub fn bad_request(title: &str, hint: &str) -> Self {
        Self::new("bad_request", title, hint)
    }
}

impl fmt::Display for UserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.title, self.hint)
    }
}

impl std::error::Error for UserError {}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "that file".into())
}

impl From<IdError> for UserError {
    fn from(_: IdError) -> Self {
        Self::bad_request(
            "Folyvo didn't recognise that file or version",
            "Close this window and open it again.",
        )
    }
}

impl From<CoreError> for UserError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::BadService(_) => {
                Self::bad_request("Folyvo didn't recognise that app", "Try sending the copy again.")
            }
            CoreError::ItemMissing => Self::new(
                "item_missing",
                "Folyvo can't find this file",
                "It may have been removed from a watched folder.",
            ),
            CoreError::AmbiguousVersion(_) => Self::bad_request(
                "More than one version matches",
                "Pick the version from the history instead.",
            ),
            CoreError::VersionMissing(_)
            | CoreError::NoSuchVersionNumber(_)
            | CoreError::NoSuchVersion(_) => Self::new(
                "version_missing",
                "That version no longer exists",
                "Open the history again to see the versions that are saved.",
            ),
            CoreError::ObjectMissing(_) | CoreError::Corrupt(_) => Self::new(
                "vault_damaged",
                "A saved copy is damaged",
                "Your file on disk is not affected. Restore a different version.",
            ),
            CoreError::Unstable(_) => {
                Self::new("file_changing", "The file is still changing", "Try again in a moment.")
            }
            CoreError::TooLarge { .. } => Self::new(
                "too_large",
                "This file is too large for the demo",
                "Files over 1 GB aren't saved yet.",
            ),
            CoreError::NotAFile(_) => Self::new(
                "not_a_file",
                "Folders and packages aren't saved yet",
                "Folyvo saves single files only.",
            ),
            CoreError::RootOverlapsVault(_) => Self::new(
                "folder_not_allowed",
                "That folder can't be watched",
                "It contains Folyvo's own storage. Choose a different folder.",
            ),
            CoreError::Watch(_) => Self::new(
                "cannot_watch",
                "Folyvo can't watch that folder",
                "Check that the folder still exists, then choose it again.",
            ),
            CoreError::Io { path, source } => match source.kind() {
                ErrorKind::PermissionDenied => Self::new(
                    "permission_denied",
                    &format!("Folyvo isn't allowed to open {}", file_name(&path)),
                    "Allow Folyvo in System Settings, Privacy & Security, Files and Folders.",
                ),
                _ => Self::new(
                    "file_unavailable",
                    &format!("Folyvo can't reach {}", file_name(&path)),
                    "Check that the file or folder still exists.",
                ),
            },
            CoreError::PathTaken(_) | CoreError::Db(_) => Self::internal(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use folyvo_core::model::{ContentHash, IdError, VersionId};
    use std::io;
    use std::path::PathBuf;

    fn code(e: CoreError) -> String {
        UserError::from(e).code
    }

    #[test]
    fn each_core_error_has_a_stable_code() {
        let v = VersionId::from_suffix("01");
        assert_eq!(code(CoreError::ItemMissing), "item_missing");
        assert_eq!(code(CoreError::VersionMissing(v)), "version_missing");
        assert_eq!(code(CoreError::NoSuchVersionNumber(9)), "version_missing");
        assert_eq!(code(CoreError::ObjectMissing(ContentHash::of_bytes(b"x"))), "vault_damaged");
        assert_eq!(code(CoreError::Corrupt("x".into())), "vault_damaged");
        assert_eq!(code(CoreError::Unstable(PathBuf::from("/a/b.docx"))), "file_changing");
        assert_eq!(code(CoreError::TooLarge { size: 2, limit: 1 }), "too_large");
        assert_eq!(code(CoreError::NotAFile(PathBuf::from("/a/R.pages"))), "not_a_file");
        assert_eq!(code(CoreError::RootOverlapsVault(PathBuf::from("/v"))), "folder_not_allowed");
        assert_eq!(code(CoreError::Watch("x".into())), "cannot_watch");
        assert_eq!(code(CoreError::BadService("sent".into())), "bad_request");
        assert_eq!(code(CoreError::NoSuchVersion("ffff".into())), "version_missing");
        assert_eq!(code(CoreError::AmbiguousVersion("abcd".into())), "bad_request");
        assert_eq!(code(CoreError::PathTaken(PathBuf::from("/a"))), "internal");
    }

    #[test]
    fn io_errors_are_split_into_permission_and_unreachable() {
        let denied = UserError::from(CoreError::io(
            "/Users/a/Desktop/x.docx",
            io::Error::from(io::ErrorKind::PermissionDenied),
        ));
        assert_eq!(denied.code, "permission_denied");
        assert!(denied.hint.contains("System Settings"), "{}", denied.hint);
        let gone = UserError::from(CoreError::io(
            "/Users/a/Desktop/x.docx",
            io::Error::from(io::ErrorKind::NotFound),
        ));
        assert_eq!(gone.code, "file_unavailable");
        assert!(gone.title.contains("x.docx"), "{}", gone.title);
    }

    #[test]
    fn nothing_technical_leaks_and_the_style_holds() {
        let samples = vec![
            CoreError::ItemMissing,
            CoreError::Corrupt("table versions has no row".into()),
            CoreError::Watch("inotify limit".into()),
            CoreError::io("/a/b.docx", io::Error::other("boom")),
            CoreError::PathTaken(PathBuf::from("/a")),
        ];
        for e in samples {
            let u = UserError::from(e);
            assert!(!u.title.is_empty() && !u.hint.is_empty());
            assert!(!u.title.ends_with('.'), "title {:?}", u.title);
            assert!(u.hint.ends_with('.'), "hint {:?}", u.hint);
            for banned in ["sqlite", "table", "inotify", "boom", "Err(", "{", "unwrap"] {
                assert!(!u.title.contains(banned) && !u.hint.contains(banned), "{banned} leaked in {u:?}");
            }
        }
    }

    #[test]
    fn a_malformed_id_is_a_bad_request() {
        let e = ItemIdParse::parse("nonsense");
        assert_eq!(UserError::from(e).code, "bad_request");
    }

    struct ItemIdParse;
    impl ItemIdParse {
        fn parse(s: &str) -> IdError {
            folyvo_core::model::ItemId::parse(s).unwrap_err()
        }
    }
}
