use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use tokio::sync::broadcast;

use crate::canonical::canonical_key;
use crate::cas::Cas;
use crate::error::{CoreError, Result};
use crate::export::{ExportResult, Exporter};
use crate::ignore::{is_ignored, is_ignored_under};
use crate::ingest::{IngestOutcome, Ingestor};
use crate::model::{ContentHash, ItemId, ItemStatus, Version, VersionId, VersionOrigin};
use crate::ports::{Clock, IdGen};
use crate::reconcile;
use crate::restore::Restorer;
use crate::share;
use crate::store::{repo, Store};

pub type EventReceiver = broadcast::Receiver<EngineEvent>;

pub const EVENT_CAPACITY: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    VersionCreated {
        item: ItemId,
        version: VersionId,
    },
    ShareReadyChanged {
        item: ItemId,
    },
    Exported {
        item: ItemId,
        destination: PathBuf,
    },
    ItemMissing {
        item: ItemId,
    },
    /// Something other than a new version changed: the item moved, or came back after being missing.
    ItemUpdated {
        item: ItemId,
    },
    RootsChanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemSummary {
    pub item_id: ItemId,
    pub name: String,
    pub path: Option<String>,
    pub status: ItemStatus,
    pub latest_seq: i64,
    pub latest_at: i64,
    pub missing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionView {
    pub id: VersionId,
    pub seq: i64,
    pub created_at: i64,
    pub size: u64,
    pub content_hash: ContentHash,
    pub origin: VersionOrigin,
    pub note: Option<String>,
    pub is_share_ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultStats {
    pub items: u64,
    pub versions: u64,
    pub bytes: u64,
}

pub(crate) struct Tracked {
    pub item: ItemId,
    pub path: PathBuf,
    pub latest: Version,
    pub missing: bool,
}

struct Inner {
    store: Store,
    cas: Cas,
}

pub struct Engine {
    vault: PathBuf,
    vault_key: String,
    inner: Mutex<Inner>,
    status: RwLock<HashMap<String, ItemStatus>>,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
    events: broadcast::Sender<EngineEvent>,
}

/// Resolve a path as far as the file system allows: a folder that no longer exists still maps
/// to the same text it had while it existed.
fn real_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| {
        match (path.parent().and_then(|p| fs::canonicalize(p).ok()), path.file_name()) {
            (Some(parent), Some(name)) => parent.join(name),
            _ => path.to_path_buf(),
        }
    })
}

impl Engine {
    pub fn open(vault: &Path, clock: Arc<dyn Clock>, ids: Arc<dyn IdGen>) -> Result<Engine> {
        fs::create_dir_all(vault).map_err(|e| CoreError::io(vault, e))?;
        let cas = Cas::open(vault)?;
        cas.sweep_tmp()?;
        let store = Store::open(&vault.join("folyvo.db"))?;
        let vault_key = canonical_key(vault)?;
        // Objects without a database row are leftovers of a crash between the object write and
        // the version insert. Nothing is ingesting yet, so removing them is safe.
        let known: HashSet<String> = repo::all_object_hashes(store.conn())?.into_iter().collect();
        for hash in cas.object_hashes()? {
            if !known.contains(hash.as_str()) {
                cas.remove_object(&hash)?;
            }
        }
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let engine = Engine {
            vault: vault.to_path_buf(),
            vault_key,
            inner: Mutex::new(Inner { store, cas }),
            status: RwLock::new(HashMap::new()),
            clock,
            ids,
            events,
        };
        {
            let g = engine.lock();
            let items: Vec<ItemId> =
                repo::all_current(g.store.conn())?.into_iter().map(|(i, _, _)| i).collect();
            for item in &items {
                engine.refresh_status(&g, item)?;
            }
        }
        Ok(engine)
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn emit(&self, event: EngineEvent) {
        let _ = self.events.send(event); // no listeners is fine
    }

    /// Recompute the cached badge for one item. Missing items have no badge.
    fn refresh_status(&self, inner: &Inner, item: &ItemId) -> Result<()> {
        let c = inner.store.conn();
        let Some(key) = repo::current_key(c, item)? else {
            return Ok(());
        };
        let missing = repo::item_flags(c, item)?.map(|(_, m)| m).unwrap_or(false);
        let status = if missing { None } else { Some(share::item_status(&inner.store, item)?) };
        let mut map = self.status.write().unwrap_or_else(|p| p.into_inner());
        match status {
            Some(st) => {
                map.insert(key, st);
            }
            None => {
                map.remove(&key);
            }
        }
        Ok(())
    }

    pub fn subscribe(&self) -> EventReceiver {
        self.events.subscribe()
    }

    pub fn add_root(&self, path: &Path) -> Result<()> {
        let root = fs::canonicalize(path).map_err(|e| CoreError::io(path, e))?;
        if !root.is_dir() {
            return Err(CoreError::io(&root, std::io::Error::other("not a folder")));
        }
        let vault = fs::canonicalize(&self.vault).map_err(|e| CoreError::io(&self.vault, e))?;
        if root.starts_with(&vault) || vault.starts_with(&root) {
            return Err(CoreError::RootOverlapsVault(root));
        }
        let g = self.lock();
        repo::insert_root(g.store.conn(), &root.to_string_lossy(), self.clock.now_unix_ms())?;
        drop(g);
        self.emit(EngineEvent::RootsChanged);
        Ok(())
    }

    /// Works for a root whose folder is gone or unmounted: it is removed under the text it was
    /// stored with, however the caller spells the path.
    pub fn remove_root(&self, path: &Path) -> Result<()> {
        let g = self.lock();
        repo::delete_root(g.store.conn(), &real_path(path).to_string_lossy())?;
        repo::delete_root(g.store.conn(), &path.to_string_lossy())?;
        drop(g);
        self.emit(EngineEvent::RootsChanged);
        Ok(())
    }

    pub fn roots(&self) -> Result<Vec<PathBuf>> {
        let g = self.lock();
        Ok(repo::list_roots(g.store.conn())?.into_iter().map(PathBuf::from).collect())
    }

    /// Should this path never be versioned? Junk names, anything inside a hidden or package
    /// folder below a watched root, the vault itself and earlier export destinations.
    pub(crate) fn is_skipped_by_policy(&self, path: &Path) -> Result<bool> {
        let key = canonical_key(path)?;
        if Path::new(&key).starts_with(&self.vault_key) {
            return Ok(true);
        }
        let g = self.lock();
        let c = g.store.conn();
        let mut ignored = None;
        for root in repo::list_roots(c)? {
            if let Ok(root_key) = canonical_key(Path::new(&root)) {
                if Path::new(&key).starts_with(&root_key) {
                    ignored = Some(is_ignored_under(Path::new(&root_key), Path::new(&key)));
                    break;
                }
            }
        }
        if ignored.unwrap_or_else(|| is_ignored(path)) || crate::dataless::is_dataless(path) {
            return Ok(true); // hashing a placeholder would force an iCloud download
        }
        repo::is_export_target(c, &key)
    }

    /// `Ok(None)` means the path is deliberately not versioned (see `is_skipped_by_policy`).
    /// A file that vanished before the debounce fired is an `Io` error; the caller treats a
    /// not-found as "mark the item missing".
    pub fn ingest_path(&self, path: &Path) -> Result<Option<IngestOutcome>> {
        if self.is_skipped_by_policy(path)? {
            return Ok(None);
        }
        let key = canonical_key(path)?;
        let mut g = self.lock();
        let inner = &mut *g;
        let was_missing = match repo::item_at_key(inner.store.conn(), &key)? {
            Some(item) => repo::item_flags(inner.store.conn(), &item)?.map(|(_, m)| m).unwrap_or(false),
            None => false,
        };
        let outcome = Ingestor::new(&mut inner.store, &inner.cas, self.clock.as_ref(), self.ids.as_ref())
            .ingest(path)?;
        let item = outcome.version().item_id.clone();
        self.refresh_status(inner, &item)?;
        match &outcome {
            IngestOutcome::Created(v) => {
                self.emit(EngineEvent::VersionCreated { item, version: v.id.clone() })
            }
            IngestOutcome::Unchanged(_) if was_missing => self.emit(EngineEvent::ItemUpdated { item }),
            IngestOutcome::Unchanged(_) => {}
        }
        Ok(Some(outcome))
    }

    /// Report that `path` no longer exists. Works even when a whole folder was deleted (so the
    /// path cannot be canonicalised any more) by falling back to the stored path text.
    pub fn mark_missing(&self, path: &Path) -> Result<()> {
        let item = {
            let g = self.lock();
            let by_key = match canonical_key(path) {
                Ok(key) => repo::item_at_key(g.store.conn(), &key)?,
                Err(_) => None,
            };
            match by_key {
                Some(item) => Some(item),
                None => repo::item_at_path(g.store.conn(), &path.to_string_lossy())?,
            }
        };
        match item {
            Some(item) => self.mark_item_missing(&item),
            None => Ok(()),
        }
    }

    pub fn mark_item_missing(&self, item: &ItemId) -> Result<()> {
        let mut g = self.lock();
        let inner = &mut *g;
        let Some(key) = repo::current_key(inner.store.conn(), item)? else {
            return Err(CoreError::ItemMissing);
        };
        let already = repo::item_flags(inner.store.conn(), item)?.map(|(_, m)| m).unwrap_or(false);
        repo::set_missing(inner.store.conn(), item, true)?;
        self.status.write().unwrap_or_else(|p| p.into_inner()).remove(&key);
        if !already {
            self.emit(EngineEvent::ItemMissing { item: item.clone() });
        }
        Ok(())
    }

    pub fn move_item(&self, item: &ItemId, new_path: &Path) -> Result<()> {
        let mut g = self.lock();
        let inner = &mut *g;
        let old_key = repo::current_key(inner.store.conn(), item)?;
        reconcile::relocate(&mut inner.store, self.clock.as_ref(), item, new_path)?;
        if let Some(old) = old_key {
            self.status.write().unwrap_or_else(|p| p.into_inner()).remove(&old);
        }
        self.refresh_status(inner, item)?;
        self.emit(EngineEvent::ItemUpdated { item: item.clone() });
        Ok(())
    }

    pub fn item_status(&self, path: &Path) -> Result<Option<ItemStatus>> {
        // A path that cannot be resolved (its folder is gone) simply has no badge. This never
        // touches the writer lock, so Finder lookups do not wait behind a large save.
        let Ok(key) = canonical_key(path) else {
            return Ok(None);
        };
        Ok(self.status.read().unwrap_or_else(|p| p.into_inner()).get(&key).copied())
    }

    fn summarize(inner: &Inner, item: &ItemId) -> Result<ItemSummary> {
        let c = inner.store.conn();
        let (name, missing) = repo::item_flags(c, item)?.ok_or(CoreError::ItemMissing)?;
        let latest = repo::latest_version(c, item)?
            .ok_or_else(|| CoreError::Corrupt(format!("{item} has no versions")))?;
        Ok(ItemSummary {
            item_id: item.clone(),
            name,
            path: repo::current_path(c, item)?,
            status: share::item_status(&inner.store, item)?,
            latest_seq: latest.seq,
            latest_at: latest.created_at,
            missing,
        })
    }

    pub fn list_recent(&self, limit: u32) -> Result<Vec<ItemSummary>> {
        let g = self.lock();
        repo::recent_items(g.store.conn(), limit)?.iter().map(|i| Self::summarize(&g, i)).collect()
    }

    pub fn history(&self, item: &ItemId) -> Result<Vec<VersionView>> {
        let g = self.lock();
        let c = g.store.conn();
        if !repo::item_exists(c, item)? {
            return Err(CoreError::ItemMissing);
        }
        let ready = repo::share_ready_version(c, item)?;
        Ok(repo::versions_newest_first(c, item)?
            .into_iter()
            .map(|v| VersionView {
                is_share_ready: ready.as_ref() == Some(&v.id),
                id: v.id,
                seq: v.seq,
                created_at: v.created_at,
                size: v.size,
                content_hash: v.content_hash,
                origin: v.origin,
                note: v.note,
            })
            .collect())
    }

    pub fn name_version(&self, version: &VersionId, note: Option<&str>) -> Result<()> {
        let g = self.lock();
        if repo::set_note(g.store.conn(), version, note)? {
            Ok(())
        } else {
            Err(CoreError::VersionMissing(version.clone()))
        }
    }

    pub fn mark_share_ready(&self, item: &ItemId, version: &VersionId) -> Result<()> {
        let mut g = self.lock();
        let inner = &mut *g;
        let before = repo::share_ready_version(inner.store.conn(), item)?;
        share::mark_share_ready(&mut inner.store, self.clock.as_ref(), item, version)?;
        if before.as_ref() != Some(version) {
            self.refresh_status(inner, item)?;
            self.emit(EngineEvent::ShareReadyChanged { item: item.clone() });
        }
        Ok(())
    }

    pub fn export(&self, item: &ItemId, expected: &VersionId, dest_dir: &Path) -> Result<ExportResult> {
        let mut g = self.lock();
        let inner = &mut *g;
        let result = Exporter::new(&mut inner.store, &inner.cas, self.clock.as_ref())
            .export(item, expected, dest_dir)?;
        self.emit(EngineEvent::Exported { item: item.clone(), destination: result.destination.clone() });
        Ok(result)
    }

    pub fn restore(&self, item: &ItemId, version: &VersionId) -> Result<Version> {
        let mut g = self.lock();
        let inner = &mut *g;
        let before = repo::latest_version(inner.store.conn(), item)?.map(|v| v.seq).unwrap_or(0);
        let restored = Restorer::new(&mut inner.store, &inner.cas, self.clock.as_ref(), self.ids.as_ref())
            .restore(item, version)?;
        self.refresh_status(inner, item)?;
        // Unsaved work captured before the overwrite is a version too: announce every new one, oldest first.
        let mut created: Vec<Version> = repo::versions_newest_first(inner.store.conn(), item)?
            .into_iter()
            .filter(|v| v.seq > before)
            .collect();
        created.reverse();
        for v in created {
            self.emit(EngineEvent::VersionCreated { item: item.clone(), version: v.id });
        }
        Ok(restored)
    }

    pub fn vault_stats(&self) -> Result<VaultStats> {
        let g = self.lock();
        let (items, versions, bytes) = repo::counts(g.store.conn())?;
        Ok(VaultStats { items, versions, bytes })
    }

    pub fn object_path(&self, hash: &ContentHash) -> PathBuf {
        self.lock().cas.path_of(hash)
    }

    pub(crate) fn tracked_under(&self, root: &Path) -> Result<Vec<Tracked>> {
        let root_key = canonical_key(root)?;
        let g = self.lock();
        let c = g.store.conn();
        let mut out = Vec::new();
        for (item, key, path) in repo::all_current(c)? {
            if !Path::new(&key).starts_with(&root_key) {
                continue;
            }
            let latest = repo::latest_version(c, &item)?
                .ok_or_else(|| CoreError::Corrupt(format!("{item} has no versions")))?;
            let missing = repo::item_flags(c, &item)?.map(|(_, m)| m).unwrap_or(false);
            out.push(Tracked { item, path: PathBuf::from(path), latest, missing });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::VersionOrigin;
    use crate::ports::{FakeClock, SeqIdGen};
    use std::fs;
    use tokio::sync::broadcast::Receiver;

    struct T {
        dir: tempfile::TempDir,
        engine: Engine,
        clock: Arc<FakeClock>,
        rx: Receiver<EngineEvent>,
    }

    fn setup() -> T {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("work")).unwrap();
        let clock = Arc::new(FakeClock::new(1_000));
        let engine =
            Engine::open(&dir.path().join("vault"), clock.clone(), Arc::new(SeqIdGen::new())).unwrap();
        let rx = engine.subscribe();
        T { dir, engine, clock, rx }
    }

    impl T {
        fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let p = self.dir.path().join("work").join(name);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, bytes).unwrap();
            p
        }
        fn events(&mut self) -> Vec<EngineEvent> {
            let mut out = Vec::new();
            while let Ok(e) = self.rx.try_recv() {
                out.push(e);
            }
            out
        }
        fn save(&mut self, p: &Path, bytes: &[u8]) -> Version {
            fs::write(p, bytes).unwrap();
            self.clock.advance(1_000);
            self.engine.ingest_path(p).unwrap().unwrap().version().clone()
        }
    }

    #[test]
    fn first_save_emits_version_created_and_a_repeat_save_emits_nothing() {
        let mut t = setup();
        let p = t.file("a.docx", b"one");
        let v = t.engine.ingest_path(&p).unwrap().unwrap().version().clone();
        assert_eq!(
            t.events(),
            vec![EngineEvent::VersionCreated { item: v.item_id.clone(), version: v.id.clone() }]
        );
        assert_eq!(t.engine.item_status(&p).unwrap(), Some(ItemStatus::NotShared));
        t.engine.ingest_path(&p).unwrap();
        assert!(t.events().is_empty());
    }

    #[test]
    fn ignored_files_and_vault_files_are_never_ingested() {
        let mut t = setup();
        let junk = t.file(".DS_Store", b"x");
        assert!(t.engine.ingest_path(&junk).unwrap().is_none());
        let inside_vault = t.dir.path().join("vault").join("note.txt");
        fs::write(&inside_vault, b"x").unwrap();
        assert!(t.engine.ingest_path(&inside_vault).unwrap().is_none());
        assert!(t.events().is_empty());
        assert_eq!(t.engine.vault_stats().unwrap().items, 0);
    }

    #[test]
    fn marking_share_ready_emits_an_event_and_the_badge_follows_later_saves() {
        let mut t = setup();
        let p = t.file("a.docx", b"one");
        let v1 = t.save(&p, b"one");
        t.events();
        t.engine.mark_share_ready(&v1.item_id, &v1.id).unwrap();
        assert_eq!(t.events(), vec![EngineEvent::ShareReadyChanged { item: v1.item_id.clone() }]);
        assert_eq!(t.engine.item_status(&p).unwrap(), Some(ItemStatus::Ready));
        t.save(&p, b"two");
        assert_eq!(t.engine.item_status(&p).unwrap(), Some(ItemStatus::ChangedSinceReady));
        t.engine.mark_share_ready(&v1.item_id, &v1.id).unwrap();
        assert!(
            t.events().iter().all(|e| !matches!(e, EngineEvent::ShareReadyChanged { .. })),
            "re-marking the same version is silent"
        );
    }

    #[test]
    fn unknown_paths_have_no_status() {
        let t = setup();
        let p = t.file("never-saved.docx", b"x");
        assert_eq!(t.engine.item_status(&p).unwrap(), None);
    }

    #[test]
    fn history_is_newest_first_flags_the_marked_version_and_shows_names() {
        let mut t = setup();
        let p = t.file("a.docx", b"one");
        let v1 = t.save(&p, b"one");
        let v2 = t.save(&p, b"two");
        t.engine.mark_share_ready(&v1.item_id, &v1.id).unwrap();
        t.engine.name_version(&v1.id, Some("Sent to Acme")).unwrap();
        let h = t.engine.history(&v1.item_id).unwrap();
        assert_eq!(h.iter().map(|v| v.seq).collect::<Vec<_>>(), vec![2, 1]);
        assert_eq!(h[0].id, v2.id);
        assert!(!h[0].is_share_ready && h[1].is_share_ready);
        assert_eq!(h[1].note.as_deref(), Some("Sent to Acme"));
        t.engine.name_version(&v1.id, None).unwrap();
        assert_eq!(t.engine.history(&v1.item_id).unwrap()[1].note, None);
        assert!(matches!(
            t.engine.name_version(&VersionId::from_suffix("dead"), Some("x")),
            Err(CoreError::VersionMissing(_))
        ));
    }

    #[test]
    fn export_emits_an_event_and_the_copy_is_not_picked_up_as_a_new_item() {
        let mut t = setup();
        let p = t.file("a.docx", b"payload");
        let v = t.save(&p, b"payload");
        t.engine.mark_share_ready(&v.item_id, &v.id).unwrap();
        t.events();
        let out = t.dir.path().join("work").join("out");
        fs::create_dir(&out).unwrap();
        let r = t.engine.export(&v.item_id, &v.id, &out).unwrap();
        assert_eq!(
            t.events(),
            vec![EngineEvent::Exported { item: v.item_id.clone(), destination: r.destination.clone() }]
        );
        assert!(t.engine.ingest_path(&r.destination).unwrap().is_none());
        assert_eq!(t.engine.vault_stats().unwrap().items, 1);
    }

    #[test]
    fn restore_emits_version_created_and_an_identical_restore_is_silent() {
        let mut t = setup();
        let p = t.file("a.docx", b"one");
        let v1 = t.save(&p, b"one");
        let v2 = t.save(&p, b"two");
        t.events();
        let r = t.engine.restore(&v1.item_id, &v1.id).unwrap();
        assert_eq!(r.origin, VersionOrigin::Restore);
        assert_eq!(
            t.events(),
            vec![EngineEvent::VersionCreated { item: v1.item_id.clone(), version: r.id.clone() }]
        );
        t.engine.restore(&v1.item_id, &r.id).unwrap();
        assert!(t.events().is_empty());
        assert_ne!(r.id, v2.id);
    }

    #[test]
    fn a_missing_file_loses_its_badge_and_comes_back_when_it_reappears() {
        let mut t = setup();
        let p = t.file("a.docx", b"one");
        let v = t.save(&p, b"one");
        t.events();
        fs::remove_file(&p).unwrap();
        t.engine.mark_missing(&p).unwrap();
        assert_eq!(t.events(), vec![EngineEvent::ItemMissing { item: v.item_id.clone() }]);
        assert_eq!(t.engine.item_status(&p).unwrap(), None);
        assert!(t.engine.list_recent(5).unwrap()[0].missing);
        t.save(&p, b"back");
        assert_eq!(t.engine.item_status(&p).unwrap(), Some(ItemStatus::NotShared));
        assert!(!t.engine.list_recent(5).unwrap()[0].missing);
    }

    #[test]
    fn a_removal_inside_a_deleted_folder_still_marks_the_item_missing_once() {
        let mut t = setup();
        let p = t.file("proj/a.docx", b"one");
        let v = t.save(&p, b"one");
        t.events();
        fs::remove_dir_all(t.dir.path().join("work/proj")).unwrap();
        t.engine.mark_missing(&p).unwrap(); // the parent folder is gone too
        assert_eq!(t.events(), vec![EngineEvent::ItemMissing { item: v.item_id.clone() }]);
        t.engine.mark_missing(&p).unwrap();
        assert!(t.events().is_empty(), "no repeat event for an item already flagged");
        assert!(t.engine.list_recent(5).unwrap()[0].missing);
    }

    #[test]
    fn move_item_carries_the_badge_to_the_new_path() {
        let mut t = setup();
        let old = t.file("draft.docx", b"one");
        let v = t.save(&old, b"one");
        let new = old.with_file_name("final.docx");
        fs::rename(&old, &new).unwrap();
        t.engine.move_item(&v.item_id, &new).unwrap();
        assert_eq!(t.engine.item_status(&old).unwrap(), None);
        assert_eq!(t.engine.item_status(&new).unwrap(), Some(ItemStatus::NotShared));
        assert_eq!(t.engine.list_recent(5).unwrap()[0].name, "draft.docx");
        assert!(t.events().contains(&EngineEvent::ItemUpdated { item: v.item_id.clone() }));
    }

    #[test]
    fn roots_overlapping_the_vault_are_refused_in_every_direction() {
        let mut t = setup();
        let vault = t.dir.path().join("vault");
        for bad in [vault.clone(), vault.join("objects"), t.dir.path().to_path_buf()] {
            let err = t.engine.add_root(&bad).unwrap_err();
            assert!(matches!(err, CoreError::RootOverlapsVault(_)), "{bad:?}: {err}");
        }
        assert!(t.events().is_empty());
        let work = t.dir.path().join("work");
        t.engine.add_root(&work).unwrap();
        t.engine.add_root(&work).unwrap(); // adding twice is harmless
        assert_eq!(t.engine.roots().unwrap().len(), 1);
        assert_eq!(t.events().iter().filter(|e| **e == EngineEvent::RootsChanged).count(), 2);
        t.engine.remove_root(&work).unwrap();
        assert!(t.engine.roots().unwrap().is_empty());
    }

    #[test]
    fn a_root_that_does_not_exist_is_an_io_error() {
        let t = setup();
        assert!(matches!(t.engine.add_root(&t.dir.path().join("nope")), Err(CoreError::Io { .. })));
    }

    #[test]
    fn list_recent_orders_by_latest_save_and_honours_the_limit() {
        let mut t = setup();
        let (a, b, c) = (t.file("a.docx", b"a"), t.file("b.docx", b"b"), t.file("c.docx", b"c"));
        t.save(&a, b"a");
        t.save(&b, b"b");
        t.save(&c, b"c");
        t.save(&a, b"a2"); // a is now the most recent
        let names: Vec<String> = t.engine.list_recent(2).unwrap().into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["a.docx", "c.docx"]);
    }

    #[test]
    fn vault_stats_count_items_versions_and_distinct_object_bytes() {
        let mut t = setup();
        let (a, b) = (t.file("a.docx", b"12345"), t.file("b.docx", b"12345"));
        t.save(&a, b"12345");
        t.save(&b, b"12345"); // identical bytes: one stored object
        t.save(&a, b"123");
        let s = t.engine.vault_stats().unwrap();
        assert_eq!((s.items, s.versions, s.bytes), (2, 3, 8));
    }

    #[test]
    fn reopening_the_vault_keeps_history_and_rebuilds_badges() {
        let mut t = setup();
        let p = t.file("a.docx", b"one");
        let v = t.save(&p, b"one");
        t.engine.mark_share_ready(&v.item_id, &v.id).unwrap();
        let vault = t.dir.path().join("vault");
        drop(t.engine);
        let again = Engine::open(&vault, t.clock.clone(), Arc::new(SeqIdGen::new())).unwrap();
        assert_eq!(again.item_status(&p).unwrap(), Some(ItemStatus::Ready));
        assert_eq!(again.history(&v.item_id).unwrap().len(), 1);
    }

    #[test]
    fn opening_sweeps_temp_files_left_by_a_crashed_write() {
        let t = setup();
        let leftover = t.dir.path().join("vault/tmp/crashed");
        fs::write(&leftover, b"partial").unwrap();
        let vault = t.dir.path().join("vault");
        drop(t.engine);
        Engine::open(&vault, t.clock.clone(), Arc::new(SeqIdGen::new())).unwrap();
        assert!(!leftover.exists());
    }

    #[test]
    fn hidden_folders_and_package_contents_below_a_root_are_never_versioned() {
        let t = setup();
        t.engine.add_root(&t.dir.path().join("work")).unwrap();
        for rel in [".git/x.docx", "Deck.key/Index/slide.xml", "a/.cache/y.docx"] {
            let p = t.file(rel, b"x");
            assert!(t.engine.ingest_path(&p).unwrap().is_none(), "{rel} must be skipped");
        }
        let ok = t.file("keep/z.docx", b"x");
        assert!(t.engine.ingest_path(&ok).unwrap().is_some());
        assert_eq!(t.engine.vault_stats().unwrap().items, 1);
    }

    #[test]
    fn unsaved_work_captured_by_a_restore_is_announced_before_the_restore_itself() {
        let mut t = setup();
        let p = t.file("a.docx", b"one");
        let v1 = t.save(&p, b"one");
        t.save(&p, b"two");
        fs::write(&p, b"draft").unwrap(); // typed but never ingested
        t.events();
        let r = t.engine.restore(&v1.item_id, &v1.id).unwrap();
        let ev = t.events();
        assert_eq!(ev.len(), 2, "{ev:?}");
        assert!(matches!(&ev[1], EngineEvent::VersionCreated { version, .. } if *version == r.id));
    }

    #[test]
    fn a_file_that_reappears_unchanged_announces_that_the_item_is_back() {
        let mut t = setup();
        let p = t.file("a.docx", b"one");
        let v = t.save(&p, b"one");
        t.engine.mark_missing(&p).unwrap();
        t.events();
        t.engine.ingest_path(&p).unwrap();
        assert_eq!(t.events(), vec![EngineEvent::ItemUpdated { item: v.item_id.clone() }]);
    }

    #[test]
    fn only_folders_can_become_roots_and_a_vanished_root_can_still_be_removed() {
        let t = setup();
        let file = t.file("not-a-folder.txt", b"x");
        assert!(matches!(t.engine.add_root(&file), Err(CoreError::Io { .. })));
        let gone = t.dir.path().join("work/client");
        fs::create_dir(&gone).unwrap();
        t.engine.add_root(&gone).unwrap();
        fs::remove_dir_all(&gone).unwrap();
        t.engine.remove_root(&gone).unwrap(); // the folder no longer exists
        assert!(t.engine.roots().unwrap().is_empty());
    }

    #[test]
    fn leftover_objects_without_a_version_are_swept_on_open_but_used_ones_survive() {
        let mut t = setup();
        let p = t.file("a.docx", b"kept");
        let v = t.save(&p, b"kept");
        // An object written just before a crash, with no database row:
        let orphan_hash = crate::model::ContentHash::of_bytes(b"orphan");
        let orphan_path = t.engine.object_path(&orphan_hash);
        fs::create_dir_all(orphan_path.parent().unwrap()).unwrap();
        fs::write(&orphan_path, b"orphan").unwrap();
        let vault = t.dir.path().join("vault");
        drop(t.engine);
        let again = Engine::open(&vault, t.clock.clone(), Arc::new(SeqIdGen::new())).unwrap();
        assert!(!orphan_path.exists());
        assert!(again.object_path(&v.content_hash).exists());
    }

    #[test]
    fn a_path_that_cannot_be_resolved_has_no_badge_instead_of_an_error() {
        let t = setup();
        assert_eq!(t.engine.item_status(&t.dir.path().join("gone-folder/a.docx")).unwrap(), None);
    }

    #[test]
    fn object_path_points_at_the_stored_bytes() {
        let mut t = setup();
        let p = t.file("a.docx", b"stored");
        let v = t.save(&p, b"stored");
        assert_eq!(fs::read(t.engine.object_path(&v.content_hash)).unwrap(), b"stored");
    }
}
