//! How much room Folyvo takes on this Mac (#54).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::engine::Engine;
use crate::error::Result;
use crate::model::{ContentHash, ItemId};
use crate::store::repo;

/// The room Folyvo takes, and the files that take the most of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Space {
    pub bytes: u64,
    pub files: Vec<FileSpace>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSpace {
    pub item: ItemId,
    pub name: String,
    pub saves: usize,
    pub bytes: u64,
}

/// The bytes of a stored file that no other file shares. A clone of a file that has not changed
/// since shares all of its blocks and takes nothing.
/// Where the file system cannot tell, the file's whole size.
pub(crate) fn private_size(path: &Path) -> u64 {
    #[cfg(target_os = "macos")]
    if let Some(bytes) = apfs_private_size(path) {
        return bytes;
    }
    std::fs::symlink_metadata(path).map_or(0, |m| m.len())
}

#[cfg(target_os = "macos")]
fn apfs_private_size(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    #[repr(C, packed(4))]
    struct Reply {
        length: u32,
        returned: libc::attribute_set_t,
        private_size: libc::off_t,
    }
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut ask: libc::attrlist = unsafe { std::mem::zeroed() };
    ask.bitmapcount = libc::ATTR_BIT_MAP_COUNT;
    ask.commonattr = libc::ATTR_CMN_RETURNED_ATTRS;
    ask.forkattr = libc::ATTR_CMNEXT_PRIVATESIZE;
    let mut reply: Reply = unsafe { std::mem::zeroed() };
    // SAFETY: `ask` and `reply` live across the call, and the size given is reply's own.
    let failed = unsafe {
        libc::getattrlist(
            path.as_ptr(),
            (&raw mut ask).cast(),
            (&raw mut reply).cast(),
            std::mem::size_of::<Reply>(),
            libc::FSOPT_ATTR_CMN_EXTENDED,
        )
    } != 0;
    let returned = reply.returned;
    if failed || returned.forkattr & libc::ATTR_CMNEXT_PRIVATESIZE == 0 {
        return None; // not APFS, or no answer
    }
    u64::try_from(reply.private_size).ok()
}

impl Engine {
    /// The room taken now, with the `top` files that take the most, largest first. A stored file
    /// two files share counts once in the whole, and for each of them in the list.
    pub fn space(&self, top: usize) -> Result<Space> {
        let (rows, cas_paths) = {
            let g = self.lock();
            let rows = repo::stored_files_by_item(g.store.conn())?;
            let paths: HashMap<ContentHash, PathBuf> =
                rows.iter().map(|r| (r.hash.clone(), g.cas.path_of(&r.hash))).collect();
            (rows, paths)
        };
        // Measured outside the lock: saves go on being kept meanwhile.
        let sizes: HashMap<&ContentHash, u64> = cas_paths.iter().map(|(h, p)| (h, private_size(p))).collect();
        let bytes = sizes.values().sum();
        let mut by_item: HashMap<&ItemId, FileSpace> = HashMap::new();
        for r in &rows {
            let file = by_item.entry(&r.item).or_insert_with(|| FileSpace {
                item: r.item.clone(),
                name: r.name.clone(),
                saves: 0,
                bytes: 0,
            });
            file.saves += r.saves;
            file.bytes += sizes[&r.hash];
        }
        let mut files: Vec<FileSpace> = by_item.into_values().filter(|f| f.bytes > 0).collect();
        files.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
        files.truncate(top);
        Ok(Space { bytes, files })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{FakeClock, SeqIdGen};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;

    struct T {
        dir: tempfile::TempDir,
        engine: Engine,
        clock: Arc<FakeClock>,
    }

    fn setup() -> T {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("work")).unwrap();
        let clock = Arc::new(FakeClock::new(1_000));
        let engine =
            Engine::open(&dir.path().join("vault"), clock.clone(), Arc::new(SeqIdGen::new())).unwrap();
        T { dir, engine, clock }
    }

    impl T {
        fn save(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let p = self.dir.path().join("work").join(name);
            fs::write(&p, bytes).unwrap();
            self.clock.advance(1_000);
            self.engine.ingest_path(&p).unwrap().unwrap();
            p
        }
    }

    /// Where stored files are clones, an unchanged file's latest save takes nothing.
    fn cloned() -> bool {
        cfg!(target_os = "macos")
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn a_file_that_never_changed_takes_no_room() {
        let t = setup();
        t.save("a.docx", &vec![7u8; 64 * 1024]);
        assert_eq!(t.engine.space(10).unwrap().bytes, 0);
    }

    #[test]
    fn a_save_the_file_has_moved_on_from_takes_its_whole_size() {
        let t = setup();
        let first = vec![1u8; 64 * 1024];
        let second = vec![2u8; 64 * 1024];
        t.save("a.docx", &first);
        t.save("a.docx", &second);
        let latest = if cloned() { 0 } else { second.len() as u64 };
        assert_eq!(t.engine.space(10).unwrap().bytes, first.len() as u64 + latest);
    }

    #[test]
    fn the_files_that_take_the_most_come_first_with_their_saves() {
        let t = setup();
        for n in 0..3u8 {
            t.save("big.pptx", &vec![n; 128 * 1024]);
        }
        t.save("small.docx", &vec![1u8; 8 * 1024]);
        t.save("small.docx", &vec![2u8; 8 * 1024]);
        let space = t.engine.space(10).unwrap();
        let files: Vec<_> = space.files.iter().map(|f| (f.name.as_str(), f.saves)).collect();
        assert_eq!(files, vec![("big.pptx", 3), ("small.docx", 2)]);
        assert!(space.files[0].bytes > space.files[1].bytes);
        assert_eq!(space.files.iter().map(|f| f.bytes).sum::<u64>(), space.bytes);
    }

    #[test]
    fn only_as_many_files_as_asked_are_listed() {
        let t = setup();
        for name in ["a.docx", "b.docx", "c.docx"] {
            t.save(name, name.repeat(4096).as_bytes());
            t.save(name, name.repeat(2048).as_bytes());
        }
        assert_eq!(t.engine.space(2).unwrap().files.len(), 2);
    }

    #[test]
    fn a_stored_file_two_files_share_counts_once() {
        let t = setup();
        let same = vec![9u8; 32 * 1024];
        t.save("a.docx", &same);
        t.save("a.docx", b"moved on");
        t.save("b.docx", &same);
        t.save("b.docx", b"moved on too");
        let space = t.engine.space(10).unwrap();
        let latest = if cloned() { 0 } else { ("moved on".len() + "moved on too".len()) as u64 };
        assert_eq!(space.bytes, same.len() as u64 + latest);
    }
}
