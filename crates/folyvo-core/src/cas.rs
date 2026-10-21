use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result};
use crate::model::ContentHash;

pub const MAX_FILE_BYTES: u64 = 1 << 30;
const CHUNK: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub hash: ContentHash,
    pub size: u64,
    pub newly_stored: bool,
}

pub struct Cas {
    root: PathBuf,
}

impl Cas {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn open(root: &Path) -> Result<Cas> {
        for sub in ["objects", "tmp"] {
            let p = root.join(sub);
            fs::create_dir_all(&p).map_err(|e| CoreError::io(&p, e))?;
        }
        restrict_to_owner(root)?;
        Ok(Cas { root: root.to_path_buf() })
    }

    pub fn path_of(&self, hash: &ContentHash) -> PathBuf {
        let (a, b) = hash.shard();
        self.root.join("objects").join(a).join(b)
    }

    pub fn contains(&self, hash: &ContentHash) -> bool {
        self.path_of(hash).is_file()
    }

    pub fn put_file(&self, src: &Path) -> Result<Stored> {
        self.put_file_with_hook(src, &mut || {})
    }

    /// `after_first_read` runs once, after the first chunk has been read. Tests use it to
    /// mutate the source mid-read; production passes a no-op.
    pub fn put_file_with_hook(&self, src: &Path, after_first_read: &mut dyn FnMut()) -> Result<Stored> {
        // Check the kind before opening: opening a directory fails on Windows with a different error.
        let kind = fs::metadata(src).map_err(|e| CoreError::io(src, e))?;
        if !kind.is_file() {
            return Err(CoreError::NotAFile(src.to_path_buf()));
        }
        let mut file = File::open(src).map_err(|e| CoreError::io(src, e))?;
        let before = file.metadata().map_err(|e| CoreError::io(src, e))?;
        if before.len() > MAX_FILE_BYTES {
            return Err(CoreError::TooLarge { size: before.len(), limit: MAX_FILE_BYTES });
        }

        let tmp_path = self.root.join("tmp").join(uuid::Uuid::new_v4().simple().to_string());
        let outcome = self.stream_into_tmp(&mut file, src, &tmp_path, after_first_read);
        let (hash, bytes_read) = match outcome {
            Ok(v) => v,
            Err(e) => {
                let _ = fs::remove_file(&tmp_path);
                return Err(e);
            }
        };

        let after = file.metadata().map_err(|e| CoreError::io(src, e))?;
        let unchanged = bytes_read == before.len()
            && after.len() == before.len()
            && after.modified().ok() == before.modified().ok();
        if !unchanged {
            let _ = fs::remove_file(&tmp_path);
            return Err(CoreError::Unstable(src.to_path_buf()));
        }

        let final_path = self.path_of(&hash);
        if final_path.is_file() {
            let _ = fs::remove_file(&tmp_path);
            return Ok(Stored { hash, size: bytes_read, newly_stored: false });
        }
        let shard_dir = final_path.parent().expect("object path has a parent").to_path_buf();
        fs::create_dir_all(&shard_dir).map_err(|e| CoreError::io(&shard_dir, e))?;
        fs::rename(&tmp_path, &final_path).map_err(|e| {
            let _ = fs::remove_file(&tmp_path);
            CoreError::io(&final_path, e)
        })?;
        sync_dir(&shard_dir);
        Ok(Stored { hash, size: bytes_read, newly_stored: true })
    }

    fn stream_into_tmp(
        &self,
        file: &mut File,
        src: &Path,
        tmp_path: &Path,
        after_first_read: &mut dyn FnMut(),
    ) -> Result<(ContentHash, u64)> {
        let mut out = File::create(tmp_path).map_err(|e| CoreError::io(tmp_path, e))?;
        let mut hasher = blake3::Hasher::new();
        let mut buf = vec![0u8; CHUNK];
        let mut total: u64 = 0;
        let mut first = true;
        loop {
            let n = file.read(&mut buf).map_err(|e| CoreError::io(src, e))?;
            if first {
                after_first_read();
                first = false;
            }
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            out.write_all(&buf[..n]).map_err(|e| CoreError::io(tmp_path, e))?;
            total += n as u64;
        }
        out.sync_all().map_err(|e| CoreError::io(tmp_path, e))?;
        Ok((ContentHash::from_blake3(hasher.finalize()), total))
    }

    pub fn verify(&self, hash: &ContentHash) -> Result<()> {
        let path = self.path_of(hash);
        let mut f = match File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(CoreError::ObjectMissing(hash.clone()))
            }
            Err(e) => return Err(CoreError::io(&path, e)),
        };
        let mut hasher = blake3::Hasher::new();
        let mut buf = vec![0u8; CHUNK];
        loop {
            let n = f.read(&mut buf).map_err(|e| CoreError::io(&path, e))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        if &ContentHash::from_blake3(hasher.finalize()) != hash {
            return Err(CoreError::Corrupt(format!("stored object {hash} no longer matches its hash")));
        }
        Ok(())
    }

    pub fn object_hashes(&self) -> Result<Vec<ContentHash>> {
        let dir = self.root.join("objects");
        let mut out = Vec::new();
        for shard in fs::read_dir(&dir).map_err(|e| CoreError::io(&dir, e))? {
            let shard = shard.map_err(|e| CoreError::io(&dir, e))?;
            if !shard.path().is_dir() {
                continue;
            }
            let prefix = shard.file_name().to_string_lossy().into_owned();
            for object in fs::read_dir(shard.path()).map_err(|e| CoreError::io(shard.path(), e))? {
                let object = object.map_err(|e| CoreError::io(shard.path(), e))?;
                let rest = object.file_name().to_string_lossy().into_owned();
                if let Ok(hash) = ContentHash::parse(&format!("b3_{prefix}{rest}")) {
                    out.push(hash);
                }
            }
        }
        Ok(out)
    }

    pub fn remove_object(&self, hash: &ContentHash) -> Result<()> {
        let path = self.path_of(hash);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(CoreError::io(&path, e)),
        }
    }

    pub fn sweep_tmp(&self) -> Result<usize> {
        let dir = self.root.join("tmp");
        let mut removed = 0;
        for entry in fs::read_dir(&dir).map_err(|e| CoreError::io(&dir, e))? {
            let entry = entry.map_err(|e| CoreError::io(&dir, e))?;
            let path = entry.path();
            if fs::remove_file(&path).is_ok() || fs::remove_dir_all(&path).is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }
}

/// The vault holds private documents: nobody but the owner may enter its directory.
#[cfg(unix)]
fn restrict_to_owner(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|e| CoreError::io(dir, e))
}

#[cfg(not(unix))]
fn restrict_to_owner(_dir: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn sync_dir(dir: &Path) {
    if let Ok(f) = File::open(dir) {
        let _ = f.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn cas() -> (tempfile::TempDir, Cas) {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::open(&dir.path().join("vault")).unwrap();
        (dir, cas)
    }

    fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = dir.path().join(name);
        fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn stores_bytes_under_their_hash_and_reads_back_identically() {
        let (dir, cas) = cas();
        let src = write(&dir, "a.docx", b"hello folyvo");
        let stored = cas.put_file(&src).unwrap();
        assert_eq!(stored.hash, ContentHash::of_bytes(b"hello folyvo"));
        assert_eq!(stored.size, 12);
        assert!(stored.newly_stored);
        assert_eq!(fs::read(cas.path_of(&stored.hash)).unwrap(), b"hello folyvo");
        assert!(cas.contains(&stored.hash));
    }

    #[test]
    fn storing_identical_content_twice_keeps_one_object() {
        let (dir, cas) = cas();
        let a = write(&dir, "a.txt", b"same");
        let b = write(&dir, "b.txt", b"same");
        let first = cas.put_file(&a).unwrap();
        let second = cas.put_file(&b).unwrap();
        assert_eq!(first.hash, second.hash);
        assert!(first.newly_stored);
        assert!(!second.newly_stored);
    }

    // Review Focus 1: an empty file is a valid file.
    #[test]
    fn empty_file_is_stored_not_rejected() {
        let (dir, cas) = cas();
        let src = write(&dir, "empty.txt", b"");
        let stored = cas.put_file(&src).unwrap();
        assert_eq!(stored.size, 0);
        assert_eq!(stored.hash, ContentHash::of_bytes(b""));
        assert!(cas.contains(&stored.hash));
    }

    // Review Focus 2 (source side): spaces and non-ASCII in the source name are irrelevant to the store.
    #[test]
    fn source_name_with_spaces_and_accents_works() {
        let (dir, cas) = cas();
        let src = write(&dir, "résumé final v2.docx", b"cv");
        assert!(cas.put_file(&src).unwrap().newly_stored);
    }

    #[test]
    fn directories_are_refused_as_not_a_file() {
        let (dir, cas) = cas();
        let sub = dir.path().join("Report.pages");
        fs::create_dir(&sub).unwrap();
        assert!(matches!(cas.put_file(&sub), Err(CoreError::NotAFile(_))));
    }

    #[test]
    fn files_over_the_size_limit_are_refused_before_any_reading() {
        let (dir, cas) = cas();
        let big = dir.path().join("huge.bin");
        let f = fs::File::create(&big).unwrap();
        f.set_len(MAX_FILE_BYTES + 1).unwrap(); // sparse, costs no real space on APFS/ext4
        match cas.put_file(&big) {
            Err(CoreError::TooLarge { size, limit }) => {
                assert_eq!(size, MAX_FILE_BYTES + 1);
                assert_eq!(limit, MAX_FILE_BYTES);
            }
            other => panic!("expected TooLarge, got {other:?}"),
        }
    }

    #[test]
    fn verify_accepts_intact_objects_and_rejects_damaged_or_missing_ones() {
        let (dir, cas) = cas();
        let stored = cas.put_file(&write(&dir, "a.docx", b"intact")).unwrap();
        cas.verify(&stored.hash).unwrap();
        fs::write(cas.path_of(&stored.hash), b"bit rot").unwrap();
        assert!(matches!(cas.verify(&stored.hash), Err(CoreError::Corrupt(_))));
        let other = ContentHash::of_bytes(b"never stored");
        assert!(matches!(cas.verify(&other), Err(CoreError::ObjectMissing(_))));
    }

    #[test]
    fn object_hashes_lists_everything_and_remove_object_deletes_one() {
        let (dir, cas) = cas();
        let a = cas.put_file(&write(&dir, "a.txt", b"aaa")).unwrap().hash;
        let b = cas.put_file(&write(&dir, "b.txt", b"bbb")).unwrap().hash;
        let mut listed = cas.object_hashes().unwrap();
        listed.sort();
        let mut expected = vec![a.clone(), b.clone()];
        expected.sort();
        assert_eq!(listed, expected);
        cas.remove_object(&a).unwrap();
        cas.remove_object(&a).unwrap(); // already gone: fine
        assert_eq!(cas.object_hashes().unwrap(), vec![b]);
    }

    #[test]
    fn missing_source_is_an_io_error_naming_the_path() {
        let (dir, cas) = cas();
        let err = cas.put_file(&dir.path().join("nope.txt")).unwrap_err();
        assert!(matches!(err, CoreError::Io { .. }), "{err}");
    }

    // Review Focus 3: file changes while being read.
    #[test]
    fn file_that_changes_mid_read_is_rejected_and_leaves_no_trace() {
        let (dir, cas) = cas();
        let src = write(&dir, "live.docx", b"0123456789");
        let src2 = src.clone();
        let mut hook = move || {
            let mut f = fs::OpenOptions::new().append(true).open(&src2).unwrap();
            f.write_all(b"MORE").unwrap();
        };
        let err = cas.put_file_with_hook(&src, &mut hook).unwrap_err();
        assert!(matches!(err, CoreError::Unstable(_)), "{err}");
        assert!(!cas.contains(&ContentHash::of_bytes(b"0123456789")));
        assert!(!cas.contains(&ContentHash::of_bytes(b"0123456789MORE")));
        assert_eq!(fs::read_dir(dir.path().join("vault/tmp")).unwrap().count(), 0);
    }

    #[test]
    fn sweep_removes_leftover_temp_files_and_directories() {
        let (dir, cas) = cas();
        fs::write(dir.path().join("vault/tmp/crashed"), b"partial").unwrap();
        fs::create_dir_all(dir.path().join("vault/tmp/snap-dir/inner")).unwrap();
        assert_eq!(cas.sweep_tmp().unwrap(), 2);
        assert_eq!(cas.sweep_tmp().unwrap(), 0);
    }

    #[test]
    fn root_returns_the_directory_it_was_opened_with() {
        let (dir, cas) = cas();
        assert_eq!(cas.root(), dir.path().join("vault"));
    }

    #[cfg(unix)]
    #[test]
    fn the_vault_directory_is_private_to_the_owner() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, _cas) = cas();
        let mode = fs::metadata(dir.path().join("vault")).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "{mode:o}");
    }
}
