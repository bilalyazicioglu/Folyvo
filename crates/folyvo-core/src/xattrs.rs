use std::path::Path;

/// Copy every extended attribute from `from` to `to`. Callers treat failure as best-effort:
/// a filesystem without xattrs must not make a restore fail.
#[cfg(unix)]
pub fn copy_xattrs(from: &Path, to: &Path) -> std::io::Result<()> {
    for name in xattr::list(from)? {
        // Per attribute and best effort: macOS refuses to set some system attributes on a new
        // file, and that must not drop the attributes after it (Finder tags among them).
        if let Ok(Some(value)) = xattr::get(from, &name) {
            let _ = xattr::set(to, &name, &value);
        }
    }
    Ok(())
}

/// Windows has no extended attributes to carry over.
#[cfg(not(unix))]
pub fn copy_xattrs(_from: &Path, _to: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;

    const NAME: &str = "user.folyvo.test";

    fn supported(path: &std::path::Path) -> bool {
        xattr::set(path, NAME, b"probe").is_ok()
    }

    #[test]
    fn attributes_are_copied_from_one_file_to_another() {
        if !xattr::SUPPORTED_PLATFORM {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        fs::write(&a, b"x").unwrap();
        fs::write(&b, b"y").unwrap();
        if !supported(&a) {
            eprintln!("skipped: this filesystem has no user xattrs");
            return;
        }
        xattr::set(&a, NAME, b"tagged").unwrap();
        copy_xattrs(&a, &b).unwrap();
        assert_eq!(xattr::get(&b, NAME).unwrap().as_deref(), Some(&b"tagged"[..]));
    }

    #[test]
    fn a_file_without_attributes_copies_nothing_and_succeeds() {
        if !xattr::SUPPORTED_PLATFORM {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        fs::write(&a, b"x").unwrap();
        fs::write(&b, b"y").unwrap();
        copy_xattrs(&a, &b).unwrap();
    }
}
