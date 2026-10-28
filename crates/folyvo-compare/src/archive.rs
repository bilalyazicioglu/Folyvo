//! Zips (Word and Excel files), opened within the limits.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use zip::result::ZipError;
use zip::{CompressionMethod, ZipArchive};

use crate::{Budget, Limits, Unreadable};

pub(crate) struct Archive {
    zip: ZipArchive<File>,
}

impl Archive {
    pub(crate) fn open(path: &Path, limits: &Limits) -> Result<Archive, Unreadable> {
        let file = File::open(path).map_err(|_| Unreadable::Malformed)?;
        let zip = ZipArchive::new(file).map_err(|_| Unreadable::Malformed)?;
        if zip.len() > limits.zip_entries {
            return Err(Unreadable::TooLarge);
        }
        Ok(Archive { zip })
    }

    #[allow(dead_code)] // the Word reader lists header and footer parts with it (next task)
    pub(crate) fn names(&self) -> Vec<String> {
        self.zip.file_names().filter_map(|n| n.ok().map(|n| n.into_owned())).collect()
    }

    pub(crate) fn has(&self, name: &str) -> bool {
        self.zip.index_for_name(name).is_some()
    }

    /// The bytes of `name`, or `None` when the zip has no such part. Stored and deflated parts
    /// only. The declared size and ratio are checked first, and reading stops one byte past the
    /// limit, so a header that lies about the size still cannot make it read more.
    #[allow(dead_code)] // the Word and Excel readers use it (next tasks)
    pub(crate) fn read_part(
        &mut self,
        name: &str,
        budget: &mut Budget,
    ) -> Result<Option<Vec<u8>>, Unreadable> {
        budget.tick()?;
        let limit = budget.limits.part_bytes;
        let ratio = budget.limits.ratio;
        let part = match self.zip.by_name(name) {
            Ok(part) => part,
            Err(ZipError::FileNotFound) => return Ok(None),
            Err(_) => return Err(Unreadable::Malformed),
        };
        if !matches!(part.compression(), CompressionMethod::Stored | CompressionMethod::Deflated) {
            return Err(Unreadable::Malformed);
        }
        if part.size() > limit || part.size() / part.compressed_size().max(1) > ratio {
            return Err(Unreadable::TooLarge);
        }
        let mut bytes = Vec::new();
        part.take(limit + 1).read_to_end(&mut bytes).map_err(|_| Unreadable::Malformed)?;
        if bytes.len() as u64 > limit {
            return Err(Unreadable::TooLarge);
        }
        budget.tick()?;
        Ok(Some(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{patch_first_entry, zip_file, zip_stored};
    use crate::{Budget, Limits, Unreadable};

    fn part(path: &std::path::Path, name: &str, limits: &Limits) -> Result<Option<Vec<u8>>, Unreadable> {
        let mut budget = Budget::new(limits);
        Archive::open(path, limits)?.read_part(name, &mut budget)
    }

    #[test]
    fn parts_read_back_and_missing_ones_are_none() {
        let d = tempfile::tempdir().unwrap();
        let z = zip_stored(d.path(), "a.zip", &[("a.xml", b"<a/>")]);
        assert_eq!(part(&z, "a.xml", &Limits::DEFAULT), Ok(Some(b"<a/>".to_vec())));
        assert_eq!(part(&z, "b.xml", &Limits::DEFAULT), Ok(None));
        assert_eq!(Archive::open(&z, &Limits::DEFAULT).unwrap().names(), vec!["a.xml".to_string()]);
    }

    #[test]
    fn too_many_entries_is_too_large() {
        let d = tempfile::tempdir().unwrap();
        let z = zip_file(d.path(), "a.zip", &[("a", b"1"), ("b", b"2"), ("c", b"3")]);
        let limits = Limits { zip_entries: 2, ..Limits::DEFAULT };
        assert_eq!(Archive::open(&z, &limits).err(), Some(Unreadable::TooLarge));
    }

    #[test]
    fn a_part_over_the_size_limit_is_too_large() {
        let d = tempfile::tempdir().unwrap();
        let z = zip_file(d.path(), "a.zip", &[("a", b"12345678901")]);
        let limits = Limits { part_bytes: 10, ..Limits::DEFAULT };
        assert_eq!(part(&z, "a", &limits), Err(Unreadable::TooLarge));
    }

    #[test]
    fn a_highly_compressed_part_is_too_large() {
        let d = tempfile::tempdir().unwrap();
        let z = zip_file(d.path(), "a.zip", &[("zeros", &vec![0u8; 1024 * 1024])]);
        assert_eq!(part(&z, "zeros", &Limits::DEFAULT), Err(Unreadable::TooLarge));
    }

    #[test]
    fn a_part_that_inflates_past_its_declared_size_is_unreadable() {
        let d = tempfile::tempdir().unwrap();
        let noise: Vec<u8> =
            (0..2 * 1024 * 1024u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
        let z = zip_file(d.path(), "a.zip", &[("big", &noise)]);
        patch_first_entry(&z, None, Some(10));
        let limits = Limits { part_bytes: 1024, ..Limits::DEFAULT };
        let got = part(&z, "big", &limits);
        assert!(matches!(got, Err(Unreadable::TooLarge) | Err(Unreadable::Malformed)), "{got:?}");
    }

    #[test]
    fn an_unknown_compression_method_is_malformed() {
        let d = tempfile::tempdir().unwrap();
        let z = zip_file(d.path(), "a.zip", &[("a", b"hello hello hello")]);
        patch_first_entry(&z, Some(14), None);
        assert_eq!(part(&z, "a", &Limits::DEFAULT), Err(Unreadable::Malformed));
    }

    #[test]
    fn not_a_zip_is_malformed() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.zip");
        std::fs::write(&p, b"PK\x03\x04 this is not a zip at all").unwrap();
        assert_eq!(Archive::open(&p, &Limits::DEFAULT).err(), Some(Unreadable::Malformed));
    }
}
