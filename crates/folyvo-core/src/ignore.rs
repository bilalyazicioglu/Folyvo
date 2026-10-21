use std::path::Path;

pub fn is_ignored(path: &Path) -> bool {
    let Some(name) = path.file_name().map(|n| n.to_string_lossy()) else {
        return true;
    };
    name.starts_with('.')
        || name.starts_with("~$")
        || name.to_lowercase().ends_with(".tmp")
        || is_excel_temp_name(&name)
}

const PACKAGE_EXTS: &[&str] =
    &["pages", "key", "numbers", "app", "bundle", "rtfd", "xcodeproj", "photoslibrary", "sparsebundle"];

pub fn is_package(path: &Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .is_some_and(|e| PACKAGE_EXTS.contains(&e.as_str()))
}

pub fn is_ignored_under(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return is_ignored(path);
    };
    let mut parts: Vec<&std::ffi::OsStr> = relative.iter().collect();
    let Some(file) = parts.pop() else {
        return true;
    };
    let in_hidden_or_package =
        parts.iter().any(|part| part.to_string_lossy().starts_with('.') || is_package(Path::new(part)));
    in_hidden_or_package || is_ignored(Path::new(file))
}

/// Excel for Mac writes an extensionless temp file named with eight hex characters.
fn is_excel_temp_name(name: &str) -> bool {
    name.len() == 8 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn ign(name: &str) -> bool {
        is_ignored(Path::new("/work").join(name).as_path())
    }

    #[test]
    fn junk_is_ignored() {
        for name in [
            ".DS_Store",
            ".hidden",
            ".~lock.report.docx#",
            "~$Report.docx",
            "x.tmp",
            "X.TMP",
            "deadbeef",
            "0a1b2c3d",
        ] {
            assert!(ign(name), "{name} should be ignored");
        }
    }

    #[test]
    fn real_documents_are_kept_even_when_they_look_similar() {
        for name in [
            "report.docx",
            "notes",
            "deadbeef.docx",
            "12345678.txt",
            "café.docx",
            "résumé final v2.docx",
            "tmp",
            "a~$b.docx",
            "deadbee",
            "deadbeef0",
        ] {
            assert!(!ign(name), "{name} should be kept");
        }
    }

    #[test]
    fn only_the_file_name_matters_not_its_folders() {
        assert!(!is_ignored(Path::new("/work/.git-like-folder/report.docx")));
        assert!(is_ignored(Path::new("/work/sub/.DS_Store")));
    }

    #[test]
    fn a_path_without_a_file_name_is_ignored() {
        assert!(is_ignored(Path::new("/")));
    }

    #[test]
    fn anything_below_a_hidden_folder_or_inside_a_package_is_ignored_under_a_root() {
        let root = Path::new("/work");
        for p in [
            "/work/.git/x.docx",
            "/work/a/.hidden/b/c.docx",
            "/work/Deck.key/Index/slide.xml",
            "/work/Report.PAGES/data.xml",
            "/work/.DS_Store",
            "/work/~$a.docx",
        ] {
            assert!(is_ignored_under(root, Path::new(p)), "{p} should be ignored");
        }
        for p in [
            "/work/report.docx",
            "/work/sub/notes.txt",
            "/work/my.key.docx",
            "/work/keynote/a.docx",
            "/work/Slides.key",
        ] {
            assert!(!is_ignored_under(root, Path::new(p)), "{p} should be kept");
        }
    }

    #[test]
    fn outside_the_root_only_the_file_name_counts() {
        let root = Path::new("/work");
        assert!(is_ignored_under(root, Path::new("/elsewhere/.DS_Store")));
        assert!(!is_ignored_under(root, Path::new("/elsewhere/.git/x.docx")));
    }

    #[test]
    fn package_detection_is_by_extension_and_case_insensitive() {
        assert!(is_package(Path::new("/w/Deck.key")));
        assert!(is_package(Path::new("/w/Deck.KEY")));
        assert!(!is_package(Path::new("/w/Deck.docx")));
        assert!(!is_package(Path::new("/w/key")));
    }
}
