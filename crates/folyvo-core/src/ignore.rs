use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::canonical::resolved_path;
use crate::dataless::is_dataless;

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

/// The header every cache folder tag starts with: <https://bford.info/cachedir/>.
const CACHE_TAG_SIGNATURE: &[u8; 43] = b"Signature: 8a477f597d28d172789f06886806bc55";

/// Whether a folder holds output a tool made rather than someone's work. Only a name no one uses
/// for their own files or a folder that marks itself counts: `target`, `build` and `dist` can hold
/// real work, and missing a save is worse than keeping too much.
pub fn is_generated_folder(dir: &Path) -> bool {
    dir.file_name().is_some_and(|name| name.eq_ignore_ascii_case("node_modules")) || has_cache_tag(dir)
}

fn has_cache_tag(dir: &Path) -> bool {
    let tag = dir.join("CACHEDIR.TAG");
    // Only a regular file: opening a FIFO would block the scan forever.
    if !std::fs::symlink_metadata(&tag).is_ok_and(|meta| meta.is_file()) {
        return false;
    }
    if is_dataless(&tag) {
        return false; // reading a placeholder would force an iCloud download
    }
    let mut start = [0; CACHE_TAG_SIGNATURE.len()];
    File::open(&tag).and_then(|mut file| file.read_exact(&mut start)).is_ok() && &start == CACHE_TAG_SIGNATURE
}

/// Whether a folder below the root is generated. The root itself never counts: choosing a folder
/// is a clear request to keep it. Keys can be case folded, so folders are checked on the real path.
pub fn is_in_generated_folder(root_key: &Path, key: &Path, path: &Path) -> bool {
    let Ok(relative) = key.strip_prefix(root_key) else {
        return false;
    };
    let folders = relative.components().count().saturating_sub(1);
    let Ok(real) = resolved_path(path) else {
        return false;
    };
    real.parent().is_some_and(|parent| parent.ancestors().take(folders).any(is_generated_folder))
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
    fn a_cache_tag_counts_only_when_it_starts_with_the_signature() {
        let dir = tempfile::tempdir().unwrap();
        let tag = dir.path().join("CACHEDIR.TAG");
        assert!(!is_generated_folder(dir.path()));
        for (bytes, generated) in [
            (&CACHE_TAG_SIGNATURE[..], true),
            (b"Signature: 8a477f597d28d172789f06886806bc55\n# by cargo\n", true),
            (b"Signature: 8a477f597d28d172789f06886806bc5", false),
            (b"signature: 8a477f597d28d172789f06886806bc55", false),
            (b"", false),
        ] {
            std::fs::write(&tag, bytes).unwrap();
            assert_eq!(is_generated_folder(dir.path()), generated, "{:?}", String::from_utf8_lossy(bytes));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_cache_tag_that_is_not_a_regular_file_does_not_block_or_count() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("elsewhere");
        let made = std::process::Command::new("mkfifo").arg(&fifo).status().unwrap();
        assert!(made.success());
        std::os::unix::fs::symlink(&fifo, dir.path().join("CACHEDIR.TAG")).unwrap();
        let linked = dir.path().to_path_buf();

        let plain = tempfile::tempdir().unwrap();
        let made =
            std::process::Command::new("mkfifo").arg(plain.path().join("CACHEDIR.TAG")).status().unwrap();
        assert!(made.success());
        let direct = plain.path().to_path_buf();

        let (sent, got) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            sent.send((is_generated_folder(&linked), is_generated_folder(&direct))).unwrap();
        });
        let answer = got.recv_timeout(std::time::Duration::from_secs(5));
        assert_eq!(answer, Ok((false, false)), "opening the tag blocked or it counted");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_to_a_real_cache_tag_does_not_count() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("elsewhere");
        std::fs::write(&real, CACHE_TAG_SIGNATURE).unwrap();
        std::os::unix::fs::symlink(&real, dir.path().join("CACHEDIR.TAG")).unwrap();
        assert!(!is_generated_folder(dir.path()));
    }

    #[test]
    fn node_modules_counts_by_name_in_any_case_and_only_as_the_whole_name() {
        for name in ["node_modules", "Node_Modules", "NODE_MODULES"] {
            assert!(is_generated_folder(&Path::new("/work").join(name)), "{name}");
        }
        for name in ["node_modules_old", "my_node_modules", "target", "build", "dist"] {
            assert!(!is_generated_folder(&Path::new("/work").join(name)), "{name}");
        }
    }

    #[test]
    fn package_detection_is_by_extension_and_case_insensitive() {
        assert!(is_package(Path::new("/w/Deck.key")));
        assert!(is_package(Path::new("/w/Deck.KEY")));
        assert!(!is_package(Path::new("/w/Deck.docx")));
        assert!(!is_package(Path::new("/w/key")));
    }
}
