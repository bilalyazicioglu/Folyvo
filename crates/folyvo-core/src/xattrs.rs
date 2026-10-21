use std::path::Path;

/// Copy every extended attribute from `from` to `to`. Callers treat failure as best-effort:
/// a filesystem without xattrs must not make a restore fail.
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
