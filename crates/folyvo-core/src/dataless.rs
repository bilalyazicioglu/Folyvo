use std::path::Path;

/// `SF_DATALESS` from `<sys/stat.h>`: the file is an iCloud placeholder with no local data.
const SF_DATALESS: u32 = 0x4000_0000;

pub fn flags_are_dataless(flags: u32) -> bool {
    flags & SF_DATALESS != 0
}

#[cfg(target_os = "macos")]
pub fn is_dataless(path: &Path) -> bool {
    use std::os::macos::fs::MetadataExt;
    std::fs::symlink_metadata(path).map(|m| flags_are_dataless(m.st_flags())).unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
pub fn is_dataless(_path: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_dataless_bit_counts() {
        assert!(flags_are_dataless(0x4000_0000));
        assert!(flags_are_dataless(0x4000_0000 | 0x2));
        assert!(!flags_are_dataless(0));
        assert!(!flags_are_dataless(0x2));
    }

    #[test]
    fn an_ordinary_local_file_is_not_dataless() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.docx");
        std::fs::write(&p, b"x").unwrap();
        assert!(!is_dataless(&p));
    }

    #[test]
    fn a_path_that_cannot_be_inspected_is_not_treated_as_dataless() {
        assert!(!is_dataless(std::path::Path::new("/definitely/not/here.docx")));
    }
}
