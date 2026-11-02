//! The first window: one look at what Folyvo does and which folders to keep, shown once.

use std::path::{Path, PathBuf};

/// The file in the vault that says the first window has been shown.
const SHOWN: &str = "first-window-shown";

/// Someone who already keeps a folder has no need for it, whatever the file says.
pub fn should_show(keeps_a_folder: bool, shown_before: bool) -> bool {
    !keeps_a_folder && !shown_before
}

pub fn shown_before(vault: &Path) -> bool {
    vault.join(SHOWN).exists()
}

pub fn remember_shown(vault: &Path) -> std::io::Result<()> {
    std::fs::write(vault.join(SHOWN), b"")
}

/// The folders the window suggests, those that exist in `home`.
pub fn suggested(home: &Path) -> Vec<PathBuf> {
    ["Documents", "Desktop"].iter().map(|name| home.join(name)).filter(|path| path.is_dir()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_shows_only_to_someone_new_and_only_once() {
        assert!(should_show(false, false));
        assert!(!should_show(false, true), "closed without choosing: the popover still offers a folder");
        assert!(!should_show(true, false), "already keeping a folder");
    }

    #[test]
    fn having_been_shown_is_remembered_in_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!shown_before(dir.path()));
        remember_shown(dir.path()).unwrap();
        assert!(shown_before(dir.path()));
    }

    #[test]
    fn it_suggests_documents_and_desktop_when_they_exist() {
        let dir = tempfile::tempdir().unwrap();
        assert!(suggested(dir.path()).is_empty());
        std::fs::create_dir(dir.path().join("Desktop")).unwrap();
        assert_eq!(suggested(dir.path()), vec![dir.path().join("Desktop")]);
        std::fs::create_dir(dir.path().join("Documents")).unwrap();
        assert_eq!(suggested(dir.path()), vec![dir.path().join("Documents"), dir.path().join("Desktop")]);
    }
}
