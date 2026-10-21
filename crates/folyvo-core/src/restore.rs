use std::fs;
use std::path::{Path, PathBuf};

use crate::cas::Cas;
use crate::error::{CoreError, Result};
use crate::ingest::Ingestor;
use crate::model::{ItemId, Version, VersionId, VersionOrigin};
use crate::ports::{Clock, IdGen};
use crate::store::{repo, Store};

pub struct Restorer<'a> {
    store: &'a mut Store,
    cas: &'a Cas,
    clock: &'a dyn Clock,
    ids: &'a dyn IdGen,
}

impl<'a> Restorer<'a> {
    pub fn new(store: &'a mut Store, cas: &'a Cas, clock: &'a dyn Clock, ids: &'a dyn IdGen) -> Self {
        Self { store, cas, clock, ids }
    }

    pub fn restore(&mut self, item: &ItemId, version: &VersionId) -> Result<Version> {
        let (target, path) = {
            let c = self.store.conn();
            if !repo::item_exists(c, item)? {
                return Err(CoreError::ItemMissing);
            }
            let target = repo::version(c, version)?
                .filter(|v| &v.item_id == item)
                .ok_or_else(|| CoreError::VersionMissing(version.clone()))?;
            let path = repo::current_path(c, item)?.ok_or(CoreError::ItemMissing)?;
            (target, PathBuf::from(path))
        };
        self.cas.verify(&target.content_hash)?; // missing -> ObjectMissing, bit rot -> Corrupt
        let object = self.cas.path_of(&target.content_hash);

        // Unsaved work inside the debounce window must not be lost.
        if path.is_file() {
            Ingestor::new(self.store, self.cas, self.clock, self.ids).ingest(&path)?;
        }

        let latest = repo::latest_version(self.store.conn(), item)?
            .ok_or_else(|| CoreError::Corrupt(format!("{item} has no versions")))?;
        let needs_version = latest.content_hash != target.content_hash;
        if needs_version || !path.is_file() {
            write_atomically(&object, &path)?;
        }
        if !needs_version {
            repo::set_missing(self.store.conn(), item, false)?;
            return Ok(latest);
        }

        let restored = Version {
            id: VersionId::from_suffix(&self.ids.next_suffix()),
            item_id: item.clone(),
            seq: latest.seq + 1,
            content_hash: target.content_hash,
            size: target.size,
            created_at: self.clock.now_unix_ms(),
            origin: VersionOrigin::Restore,
            restored_from: Some(target.id),
            note: None,
        };
        let tx = self.store.conn_mut().transaction()?;
        repo::insert_version(&tx, &restored)?;
        repo::set_missing(&tx, item, false)?;
        tx.commit()?;
        Ok(restored)
    }
}

fn write_atomically(object: &Path, target: &Path) -> Result<()> {
    let parent =
        target.parent().ok_or_else(|| CoreError::io(target, std::io::Error::other("path has no parent")))?;
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = parent.join(format!(".{name}.folyvo-restore-{}", uuid::Uuid::new_v4().simple()));
    fs::copy(object, &tmp).map_err(|e| CoreError::io(&tmp, e))?;
    let result = (|| {
        match fs::metadata(target) {
            Ok(existing) => fs::set_permissions(&tmp, existing.permissions()),
            Err(_) => default_permissions(&tmp),
        }
        .map_err(|e| CoreError::io(&tmp, e))?;
        fs::rename(&tmp, target).map_err(|e| CoreError::io(target, e))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(unix)]
fn default_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o644))
}

#[cfg(not(unix))]
fn default_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::IngestOutcome;
    use crate::model::VersionOrigin;
    use crate::store::repo;
    use crate::testutil::Env;
    use std::fs;
    use std::path::PathBuf;

    fn restore(e: &mut Env, item: &ItemId, v: &VersionId) -> Result<Version> {
        Restorer::new(&mut e.store, &e.cas, &e.clock, &e.ids).restore(item, v)
    }

    fn history(e: &Env, item: &ItemId) -> Vec<Version> {
        repo::versions_newest_first(e.store.conn(), item).unwrap()
    }

    fn two_versions(e: &mut Env, name: &str) -> (ItemId, Version, PathBuf) {
        let p = e.file(name, b"one");
        let v1 = e.ingest(&p).unwrap().version().clone();
        fs::write(&p, b"two").unwrap();
        e.ingest(&p).unwrap();
        (v1.item_id.clone(), v1, p)
    }

    #[test]
    fn restoring_writes_the_old_bytes_and_adds_a_restore_version() {
        let mut e = Env::new();
        let (item, v1, p) = two_versions(&mut e, "a.docx");
        let r = restore(&mut e, &item, &v1.id).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"one");
        assert_eq!(r.seq, 3);
        assert_eq!(r.origin, VersionOrigin::Restore);
        assert_eq!(r.restored_from, Some(v1.id.clone()));
        assert_eq!(r.content_hash, v1.content_hash);
        assert_eq!(history(&e, &item).len(), 3, "history is never rewritten");
    }

    #[test]
    fn unsaved_work_is_captured_before_it_is_overwritten() {
        let mut e = Env::new();
        let (item, v1, p) = two_versions(&mut e, "a.docx");
        fs::write(&p, b"draft typed one second ago").unwrap(); // not ingested yet
        restore(&mut e, &item, &v1.id).unwrap();
        let h = history(&e, &item);
        assert_eq!(h.len(), 4);
        assert_eq!(h[0].origin, VersionOrigin::Restore);
        assert_eq!(h[1].content_hash, crate::model::ContentHash::of_bytes(b"draft typed one second ago"));
        assert_eq!(fs::read(&p).unwrap(), b"one");
    }

    #[test]
    fn the_watcher_sees_the_restored_file_as_unchanged() {
        let mut e = Env::new();
        let (item, v1, p) = two_versions(&mut e, "a.docx");
        restore(&mut e, &item, &v1.id).unwrap();
        let out = e.ingest(&p).unwrap();
        assert!(matches!(out, IngestOutcome::Unchanged(_)));
        assert_eq!(history(&e, &item).len(), 3);
    }

    #[test]
    fn restoring_the_content_that_is_already_current_changes_nothing() {
        let mut e = Env::new();
        let (item, _v1, p) = two_versions(&mut e, "a.docx");
        let latest = history(&e, &item)[0].clone();
        let r = restore(&mut e, &item, &latest.id).unwrap();
        assert_eq!(r, latest);
        assert_eq!(history(&e, &item).len(), 2);
        assert_eq!(fs::read(&p).unwrap(), b"two");
    }

    // Review Focus 5: the working file was deleted since the last version.
    #[test]
    fn restore_recreates_a_deleted_working_file() {
        let mut e = Env::new();
        let (item, v1, p) = two_versions(&mut e, "a.docx");
        fs::remove_file(&p).unwrap();
        repo::set_missing(e.store.conn(), &item, true).unwrap();
        let r = restore(&mut e, &item, &v1.id).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"one");
        assert_eq!(r.origin, VersionOrigin::Restore);
        let m: i64 = e.store.conn().query_row("SELECT missing FROM items", [], |r| r.get(0)).unwrap();
        assert_eq!(m, 0);
    }

    #[test]
    fn restoring_the_latest_content_into_a_deleted_file_recreates_it_without_a_new_version() {
        let mut e = Env::new();
        let (item, _v1, p) = two_versions(&mut e, "a.docx");
        let latest = history(&e, &item)[0].clone();
        fs::remove_file(&p).unwrap();
        let r = restore(&mut e, &item, &latest.id).unwrap();
        assert_eq!(r, latest);
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert_eq!(history(&e, &item).len(), 2);
    }

    #[test]
    fn a_version_of_another_item_is_refused_and_nothing_changes() {
        let mut e = Env::new();
        let (item_a, _v1, pa) = two_versions(&mut e, "a.docx");
        let pb = e.file("b.docx", b"other");
        let vb = e.ingest(&pb).unwrap().version().clone();
        let err = restore(&mut e, &item_a, &vb.id).unwrap_err();
        assert!(matches!(err, CoreError::VersionMissing(_)), "{err}");
        assert_eq!(fs::read(&pa).unwrap(), b"two");
        assert_eq!(history(&e, &item_a).len(), 2);
    }

    #[test]
    fn unknown_item_is_reported_as_missing() {
        let mut e = Env::new();
        let err = restore(&mut e, &ItemId::from_suffix("dead"), &VersionId::from_suffix("beef")).unwrap_err();
        assert!(matches!(err, CoreError::ItemMissing), "{err}");
    }

    #[test]
    fn a_missing_stored_object_leaves_the_working_file_untouched() {
        let mut e = Env::new();
        let (item, v1, p) = two_versions(&mut e, "a.docx");
        fs::remove_file(e.cas.path_of(&v1.content_hash)).unwrap();
        let err = restore(&mut e, &item, &v1.id).unwrap_err();
        assert!(matches!(err, CoreError::ObjectMissing(_)), "{err}");
        assert_eq!(fs::read(&p).unwrap(), b"two");
    }

    #[test]
    fn a_damaged_stored_object_is_refused_and_the_working_file_is_untouched() {
        let mut e = Env::new();
        let (item, v1, p) = two_versions(&mut e, "a.docx");
        fs::write(e.cas.path_of(&v1.content_hash), b"bit rot").unwrap();
        let err = restore(&mut e, &item, &v1.id).unwrap_err();
        assert!(matches!(err, CoreError::Corrupt(_)), "{err}");
        assert_eq!(fs::read(&p).unwrap(), b"two");
    }

    #[cfg(unix)]
    #[test]
    fn file_permissions_survive_a_restore() {
        use std::os::unix::fs::PermissionsExt;
        let mut e = Env::new();
        let (item, v1, p) = two_versions(&mut e, "a.docx");
        fs::set_permissions(&p, fs::Permissions::from_mode(0o640)).unwrap();
        restore(&mut e, &item, &v1.id).unwrap();
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o640);
    }

    // Review Focus 2: spaces and non-ASCII names.
    #[test]
    fn names_with_spaces_and_accents_restore_in_place_and_leave_no_temp_files() {
        let mut e = Env::new();
        let (item, v1, p) = two_versions(&mut e, "résumé final v2.docx");
        restore(&mut e, &item, &v1.id).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"one");
        let names: Vec<_> =
            fs::read_dir(p.parent().unwrap()).unwrap().map(|x| x.unwrap().file_name()).collect();
        assert_eq!(names.len(), 1, "{names:?}");
    }
}
