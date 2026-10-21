use std::fs;
use std::path::Path;

use unicode_normalization::UnicodeNormalization;

use crate::error::{CoreError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseRule {
    Fold,
    Preserve,
}

impl CaseRule {
    /// Stand-in for per-volume detection: default macOS and Windows volumes are
    /// case-insensitive. Replace with real detection once a finding records it.
    pub fn for_platform() -> CaseRule {
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            CaseRule::Fold
        } else {
            CaseRule::Preserve
        }
    }
}

pub fn canonical_key(path: &Path) -> Result<String> {
    canonical_key_with(path, CaseRule::for_platform())
}

pub fn canonical_key_with(path: &Path, rule: CaseRule) -> Result<String> {
    let resolved = match fs::canonicalize(path) {
        Ok(p) => p,
        Err(_) => {
            // The file may have just been renamed away or deleted: resolve the parent instead.
            let parent = path
                .parent()
                .ok_or_else(|| CoreError::io(path, std::io::Error::other("path has no parent")))?;
            let name = path
                .file_name()
                .ok_or_else(|| CoreError::io(path, std::io::Error::other("path has no file name")))?;
            let parent = fs::canonicalize(parent).map_err(|e| CoreError::io(parent, e))?;
            parent.join(name)
        }
    };
    let text = resolved.to_string_lossy();
    let nfc: String = text.nfc().collect();
    Ok(match rule {
        CaseRule::Fold => nfc.to_lowercase(),
        CaseRule::Preserve => nfc,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const NFC: &str = "caf\u{00e9}.docx"; // é as one code point
    const NFD: &str = "cafe\u{0301}.docx"; // e + combining acute

    #[test]
    fn nfc_and_nfd_spellings_of_one_name_share_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let a = canonical_key_with(&dir.path().join(NFC), CaseRule::Preserve).unwrap();
        let b = canonical_key_with(&dir.path().join(NFD), CaseRule::Preserve).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn fold_rule_ignores_case_and_preserve_rule_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let (lo, up) = (dir.path().join("a.docx"), dir.path().join("A.docx"));
        assert_eq!(
            canonical_key_with(&lo, CaseRule::Fold).unwrap(),
            canonical_key_with(&up, CaseRule::Fold).unwrap()
        );
        assert_ne!(
            canonical_key_with(&lo, CaseRule::Preserve).unwrap(),
            canonical_key_with(&up, CaseRule::Preserve).unwrap()
        );
    }

    #[test]
    fn a_vanished_file_keeps_the_key_it_had_while_it_existed() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("gone.docx");
        fs::write(&p, b"x").unwrap();
        let while_present = canonical_key_with(&p, CaseRule::Preserve).unwrap();
        fs::remove_file(&p).unwrap();
        assert_eq!(canonical_key_with(&p, CaseRule::Preserve).unwrap(), while_present);
    }

    #[test]
    fn key_is_the_resolved_real_path() {
        let dir = tempfile::tempdir().unwrap();
        let real = fs::canonicalize(dir.path()).unwrap();
        let key = canonical_key_with(&dir.path().join("a.docx"), CaseRule::Preserve).unwrap();
        assert!(key.starts_with(real.to_str().unwrap()), "{key} should start with {real:?}");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_and_target_share_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.docx");
        fs::write(&target, b"x").unwrap();
        let link = dir.path().join("link.docx");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(
            canonical_key_with(&link, CaseRule::Preserve).unwrap(),
            canonical_key_with(&target, CaseRule::Preserve).unwrap()
        );
    }

    #[test]
    fn a_path_whose_parent_is_missing_is_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("no-such-dir").join("a.docx");
        assert!(matches!(canonical_key_with(&p, CaseRule::Preserve), Err(CoreError::Io { .. })));
    }
}
