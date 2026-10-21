use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::canonical::canonical_key;
use crate::cas::Cas;
use crate::error::{CoreError, Result};
use crate::model::{ContentHash, ItemId, ShareEvent, ShareEventKind, VersionId};
use crate::ports::Clock;
use crate::store::{repo, Store};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportResult {
    pub destination: PathBuf,
    pub materialized: PathBuf,
    pub version_id: VersionId,
}

pub struct Exporter<'a> {
    store: &'a mut Store,
    cas: &'a Cas,
    clock: &'a dyn Clock,
}

impl<'a> Exporter<'a> {
    pub fn new(store: &'a mut Store, cas: &'a Cas, clock: &'a dyn Clock) -> Self {
        Self { store, cas, clock }
    }

    pub fn export(&mut self, item: &ItemId, expected: &VersionId, dest_dir: &Path) -> Result<ExportResult> {
        let (version, name) = {
            let c = self.store.conn();
            if !repo::item_exists(c, item)? {
                return Err(CoreError::ItemMissing);
            }
            let ready = repo::share_ready_version(c, item)?.ok_or(CoreError::ShareReadyMissing)?;
            if &ready != expected {
                return Err(CoreError::ShareReadyChanged { expected: expected.clone(), actual: ready });
            }
            let version = repo::version(c, &ready)?
                .ok_or_else(|| CoreError::Corrupt(format!("share-ready points at missing {ready}")))?;
            let name = repo::current_path(c, item)?
                .and_then(|p| Path::new(&p).file_name().map(|n| n.to_string_lossy().into_owned()))
                .or(repo::display_name(c, item)?)
                .unwrap_or_else(|| "file".to_string());
            (version, name)
        };
        if !dest_dir.is_dir() {
            return Err(CoreError::io(dest_dir, std::io::Error::from(std::io::ErrorKind::NotFound)));
        }

        let materialized = self.materialize(item, &version.content_hash, &name)?;
        let destination = copy_no_clobber(&materialized, dest_dir, &name)?;

        let now = self.clock.now_unix_ms();
        let key = canonical_key(&destination)?;
        repo::insert_event(
            self.store.conn(),
            &ShareEvent {
                item_id: item.clone(),
                version_id: version.id.clone(),
                kind: ShareEventKind::Exported,
                target_path: Some(destination.to_string_lossy().into_owned()),
                at: now,
            },
            Some(&key),
        )?;
        Ok(ExportResult { destination, materialized, version_id: version.id })
    }

    fn materialize(&self, item: &ItemId, hash: &ContentHash, name: &str) -> Result<PathBuf> {
        let source = self.cas.path_of(hash);
        if !source.is_file() {
            return Err(CoreError::ObjectMissing(hash.clone()));
        }
        let base = self.cas.root().join("exports").join(item.as_str()).join(hash.as_str());
        for generation in 1u32.. {
            let dir = base.join(format!("g{generation}"));
            let path = dir.join(name);
            if path.exists() {
                if &file_hash(&path)? == hash {
                    return Ok(path);
                }
                continue; // tampered: never rewrite, try the next generation
            }
            self.cas.verify(hash)?; // never materialise bit-rotted bytes
            fs::create_dir_all(&dir).map_err(|e| CoreError::io(&dir, e))?;
            let tmp = dir.join(format!(".part-{}", uuid::Uuid::new_v4().simple()));
            fs::copy(&source, &tmp).map_err(|e| CoreError::io(&tmp, e))?;
            make_read_only(&tmp)?;
            fs::rename(&tmp, &path).map_err(|e| {
                let _ = fs::remove_file(&tmp);
                CoreError::io(&path, e)
            })?;
            return Ok(path);
        }
        unreachable!("generation counter is unbounded")
    }
}

fn file_hash(path: &Path) -> Result<ContentHash> {
    let mut f = File::open(path).map_err(|e| CoreError::io(path, e))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| CoreError::io(path, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(ContentHash::from_blake3(hasher.finalize()))
}

/// `report.docx` -> `report (2).docx`; `README` -> `README (2)`; n == 1 keeps the name.
fn numbered(name: &str, n: u32) -> String {
    if n == 1 {
        return name.to_string();
    }
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{} ({}){}", &name[..i], n, &name[i..]),
        _ => format!("{name} ({n})"),
    }
}

/// Owner read-only. Nobody else gets access, matching the private vault.
fn make_read_only(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o400)).map_err(|e| CoreError::io(path, e))
    }
    #[cfg(not(unix))]
    {
        let mut perms = fs::metadata(path).map_err(|e| CoreError::io(path, e))?.permissions();
        perms.set_readonly(true);
        fs::set_permissions(path, perms).map_err(|e| CoreError::io(path, e))
    }
}

/// Add the owner's write bit and nothing else, so a private source never becomes group or world readable.
fn make_user_writable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path).map_err(|e| CoreError::io(path, e))?.permissions().mode();
        fs::set_permissions(path, fs::Permissions::from_mode(mode | 0o200))
            .map_err(|e| CoreError::io(path, e))
    }
    #[cfg(not(unix))]
    {
        let mut perms = fs::metadata(path).map_err(|e| CoreError::io(path, e))?.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        fs::set_permissions(path, perms).map_err(|e| CoreError::io(path, e))
    }
}

/// Copy through a temp file, then publish with a hard link, which fails if the name exists.
/// Where hard links are unsupported, fall back to a create-new copy, which also never overwrites.
fn copy_no_clobber(src: &Path, dest_dir: &Path, name: &str) -> Result<PathBuf> {
    let tmp = dest_dir.join(format!(".{}.folyvo-part", uuid::Uuid::new_v4().simple()));
    fs::copy(src, &tmp).map_err(|e| CoreError::io(&tmp, e))?;
    if let Err(e) = make_user_writable(&tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    for n in 1u32.. {
        let candidate = dest_dir.join(numbered(name, n));
        match fs::hard_link(&tmp, &candidate) {
            Ok(()) => {
                let _ = fs::remove_file(&tmp);
                return Ok(candidate);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) if candidate.exists() => continue,
            Err(_) => {
                // No hard links on this filesystem: copy into a name that must not exist yet.
                match fs::OpenOptions::new().write(true).create_new(true).open(&candidate) {
                    Ok(mut dst) => {
                        let copied = File::open(&tmp).and_then(|mut s| std::io::copy(&mut s, &mut dst));
                        let _ = fs::remove_file(&tmp);
                        return match copied {
                            Ok(_) => Ok(candidate),
                            Err(e) => {
                                let _ = fs::remove_file(&candidate);
                                Err(CoreError::io(candidate, e))
                            }
                        };
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(e) => {
                        let _ = fs::remove_file(&tmp);
                        return Err(CoreError::io(candidate, e));
                    }
                }
            }
        }
    }
    unreachable!("candidate counter is unbounded")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ShareEventKind;
    use crate::share::{mark_share_ready, share_events};
    use crate::store::repo;
    use crate::testutil::Env;
    use std::fs;
    use std::path::PathBuf;

    fn marked(e: &mut Env, name: &str, bytes: &[u8]) -> (ItemId, VersionId, PathBuf) {
        let p = e.file(name, bytes);
        let v = e.ingest(&p).unwrap().version().clone();
        mark_share_ready(&mut e.store, &e.clock, &v.item_id, &v.id).unwrap();
        (v.item_id, v.id, p)
    }

    fn dest(e: &Env) -> PathBuf {
        let d = e.dir.path().join("out");
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn export(e: &mut Env, item: &ItemId, v: &VersionId, d: &Path) -> Result<ExportResult> {
        Exporter::new(&mut e.store, &e.cas, &e.clock).export(item, v, d)
    }

    #[test]
    fn exports_the_marked_bytes_under_the_original_name_and_logs_it() {
        let mut e = Env::new();
        let (item, v, _) = marked(&mut e, "Proposal.docx", b"v1 bytes");
        let d = dest(&e);
        let r = export(&mut e, &item, &v, &d).unwrap();
        assert_eq!(r.destination, d.join("Proposal.docx"));
        assert_eq!(fs::read(&r.destination).unwrap(), b"v1 bytes");
        let log = share_events(&e.store, &item).unwrap();
        let last = log.last().unwrap();
        assert_eq!(last.kind, ShareEventKind::Exported);
        assert_eq!(last.version_id, v);
        assert_eq!(last.target_path.as_deref(), Some(r.destination.to_str().unwrap()));
    }

    #[test]
    fn exports_the_marked_version_even_when_the_file_has_moved_on() {
        let mut e = Env::new();
        let (item, v, p) = marked(&mut e, "a.docx", b"marked");
        fs::write(&p, b"newer").unwrap();
        e.ingest(&p).unwrap();
        let d = dest(&e);
        let r = export(&mut e, &item, &v, &d).unwrap();
        assert_eq!(fs::read(r.destination).unwrap(), b"marked");
    }

    #[test]
    fn nothing_marked_means_share_ready_missing() {
        let mut e = Env::new();
        let p = e.file("a.docx", b"x");
        let v = e.ingest(&p).unwrap().version().clone();
        let d = dest(&e);
        let err = export(&mut e, &v.item_id, &v.id, &d).unwrap_err();
        assert!(matches!(err, CoreError::ShareReadyMissing), "{err}");
    }

    #[test]
    fn exporting_a_version_that_is_no_longer_the_marked_one_is_refused() {
        let mut e = Env::new();
        let (item, v1, p) = marked(&mut e, "a.docx", b"one");
        fs::write(&p, b"two").unwrap();
        let v2 = e.ingest(&p).unwrap().version().clone();
        mark_share_ready(&mut e.store, &e.clock, &item, &v2.id).unwrap();
        let d = dest(&e);
        let err = export(&mut e, &item, &v1, &d).unwrap_err();
        assert!(matches!(err, CoreError::ShareReadyChanged { .. }), "{err}");
        assert_eq!(fs::read_dir(&d).unwrap().count(), 0, "nothing may be copied on refusal");
    }

    // Review Focus 4: existing names are never overwritten, read-only ones included.
    #[test]
    fn a_name_clash_gets_a_number_and_the_existing_file_is_untouched() {
        let mut e = Env::new();
        let (item, v, _) = marked(&mut e, "Report.docx", b"fresh");
        let d = dest(&e);
        let existing = d.join("Report.docx");
        fs::write(&existing, b"precious").unwrap();
        let mut perms = fs::metadata(&existing).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&existing, perms).unwrap();

        let r2 = export(&mut e, &item, &v, &d).unwrap();
        let r3 = export(&mut e, &item, &v, &d).unwrap();
        assert_eq!(r2.destination, d.join("Report (2).docx"));
        assert_eq!(r3.destination, d.join("Report (3).docx"));
        assert_eq!(fs::read(&existing).unwrap(), b"precious");
        assert_eq!(fs::read(&r2.destination).unwrap(), b"fresh");
    }

    #[test]
    fn a_name_without_an_extension_is_numbered_at_the_end() {
        let mut e = Env::new();
        let (item, v, _) = marked(&mut e, "README", b"r");
        let d = dest(&e);
        fs::write(d.join("README"), b"old").unwrap();
        assert_eq!(export(&mut e, &item, &v, &d).unwrap().destination, d.join("README (2)"));
    }

    #[test]
    fn the_users_copy_is_writable_and_no_temp_file_is_left_behind() {
        let mut e = Env::new();
        let (item, v, _) = marked(&mut e, "a.docx", b"x");
        let d = dest(&e);
        let r = export(&mut e, &item, &v, &d).unwrap();
        assert!(!fs::metadata(&r.destination).unwrap().permissions().readonly());
        let names: Vec<_> = fs::read_dir(&d).unwrap().map(|x| x.unwrap().file_name()).collect();
        assert_eq!(names.len(), 1, "{names:?}");
    }

    #[test]
    fn the_materialisation_is_read_only_item_and_hash_scoped_and_reused() {
        let mut e = Env::new();
        let (item, v, _) = marked(&mut e, "a.docx", b"payload");
        let d = dest(&e);
        let first = export(&mut e, &item, &v, &d).unwrap();
        let hash = crate::model::ContentHash::of_bytes(b"payload");
        let expected =
            e.cas.root().join("exports").join(item.as_str()).join(hash.as_str()).join("g1").join("a.docx");
        assert_eq!(first.materialized, expected);
        assert!(fs::metadata(&expected).unwrap().permissions().readonly());
        let stamp = fs::metadata(&expected).unwrap().modified().unwrap();
        let second = export(&mut e, &item, &v, &d).unwrap();
        assert_eq!(second.materialized, expected);
        assert_eq!(fs::metadata(&expected).unwrap().modified().unwrap(), stamp);
    }

    #[test]
    fn a_tampered_materialisation_is_left_alone_and_the_next_generation_is_used() {
        let mut e = Env::new();
        let (item, v, _) = marked(&mut e, "a.docx", b"truth");
        let d = dest(&e);
        let first = export(&mut e, &item, &v, &d).unwrap();
        let mut perms = fs::metadata(&first.materialized).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        fs::set_permissions(&first.materialized, perms).unwrap();
        fs::write(&first.materialized, b"tampered").unwrap();

        let second = export(&mut e, &item, &v, &d).unwrap();
        assert!(
            second.materialized.to_string_lossy().contains("/g2/")
                || second.materialized.to_string_lossy().contains("\\g2\\")
        );
        assert_eq!(fs::read(&first.materialized).unwrap(), b"tampered");
        assert_eq!(fs::read(&second.destination).unwrap(), b"truth");
    }

    // Review Focus 2: the spec's collision case, identical bytes and names that differ only in
    // Unicode normalisation. On APFS both spellings are the same name.
    #[test]
    fn identical_bytes_under_nfc_and_nfd_names_never_share_or_overwrite_files() {
        let mut e = Env::new();
        fs::create_dir_all(e.dir.path().join("work/a")).unwrap();
        fs::create_dir_all(e.dir.path().join("work/b")).unwrap();
        let (ia, va, _) = marked(&mut e, "a/caf\u{00e9}.docx", b"same bytes"); // NFC
        let (ib, vb, _) = marked(&mut e, "b/cafe\u{0301}.docx", b"same bytes"); // NFD
        let d = dest(&e);
        let ra = export(&mut e, &ia, &va, &d).unwrap();
        let rb = export(&mut e, &ib, &vb, &d).unwrap();
        for (r, item) in [(&ra, &ia), (&rb, &ib)] {
            let item_dir = r
                .materialized
                .ancestors()
                .nth(3)
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            assert_eq!(item_dir, item.as_str(), "the item id scopes the materialisation");
            assert_eq!(fs::read(&r.materialized).unwrap(), b"same bytes");
            assert_eq!(fs::read(&r.destination).unwrap(), b"same bytes");
        }
        assert_ne!(ra.destination, rb.destination, "the second export never overwrites the first");
    }

    #[test]
    fn a_damaged_stored_object_is_refused_not_exported() {
        let mut e = Env::new();
        let (item, v, _) = marked(&mut e, "a.docx", b"payload");
        let hash = crate::model::ContentHash::of_bytes(b"payload");
        fs::write(e.cas.path_of(&hash), b"bit rot").unwrap();
        let d = dest(&e);
        let err = export(&mut e, &item, &v, &d).unwrap_err();
        assert!(matches!(err, CoreError::Corrupt(_)), "{err}");
        assert_eq!(fs::read_dir(&d).unwrap().count(), 0);
    }

    #[test]
    fn a_missing_destination_folder_is_an_error_and_logs_nothing() {
        let mut e = Env::new();
        let (item, v, _) = marked(&mut e, "a.docx", b"x");
        let nowhere = e.dir.path().join("does-not-exist");
        assert!(export(&mut e, &item, &v, &nowhere).is_err());
        let log = share_events(&e.store, &item).unwrap();
        assert!(log.iter().all(|ev| ev.kind == ShareEventKind::Marked));
    }

    #[test]
    fn the_export_destination_is_remembered_so_the_watcher_can_ignore_it() {
        let mut e = Env::new();
        let (item, v, _) = marked(&mut e, "a.docx", b"x");
        let d = dest(&e);
        let r = export(&mut e, &item, &v, &d).unwrap();
        let key = crate::canonical::canonical_key(&r.destination).unwrap();
        assert!(repo::is_export_target(e.store.conn(), &key).unwrap());
        assert!(!repo::is_export_target(e.store.conn(), "somewhere/else").unwrap());
    }

    #[test]
    fn the_name_follows_a_rename_of_the_working_file() {
        let mut e = Env::new();
        let (item, v, p) = marked(&mut e, "draft.docx", b"x");
        let renamed = p.with_file_name("final.docx");
        fs::rename(&p, &renamed).unwrap();
        let key = crate::canonical::canonical_key(&renamed).unwrap();
        repo::set_current_location(e.store.conn(), &item, renamed.to_str().unwrap(), &key, 9).unwrap();
        let d = dest(&e);
        assert_eq!(export(&mut e, &item, &v, &d).unwrap().destination, d.join("final.docx"));
    }

    #[cfg(unix)]
    #[test]
    fn the_users_copy_is_private_to_the_owner_like_the_vault() {
        use std::os::unix::fs::PermissionsExt;
        let mut e = Env::new();
        let (item, v, _) = marked(&mut e, "secret.docx", b"x");
        let d = dest(&e);
        let r = export(&mut e, &item, &v, &d).unwrap();
        let mode = fs::metadata(&r.destination).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "group/other bits set: {mode:o}");
        assert_ne!(mode & 0o200, 0, "owner must be able to write: {mode:o}");
        let mat = fs::metadata(&r.materialized).unwrap().permissions().mode();
        assert_eq!(mat & 0o277, 0, "materialisation must be owner read-only: {mat:o}");
    }
}
