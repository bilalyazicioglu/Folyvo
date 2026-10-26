use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use tokio::sync::broadcast;

use crate::canonical::canonical_key;
use crate::cas::Cas;
use crate::error::{CoreError, Result};
use crate::events;
use crate::export::{ExportResult, Exporter};
use crate::handed;
use crate::ignore::{is_ignored, is_ignored_under};
use crate::ingest::{IngestOutcome, Ingestor};
use crate::lock::VaultLock;
use crate::model::{version_codes, ContentHash, ItemId, ShareEvent, Version, VersionId, VersionOrigin};
use crate::ports::{Clock, IdGen};
use crate::reconcile;
use crate::restore::Restorer;
use crate::store::{repo, Store};
use crate::which;

pub use tokio::sync::broadcast::error::RecvError;

pub type EventReceiver = broadcast::Receiver<EngineEvent>;

pub const EVENT_CAPACITY: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    VersionCreated {
        item: ItemId,
        version: VersionId,
    },
    Exported {
        item: ItemId,
        destination: PathBuf,
    },
    /// A copy went to another app through the share sheet.
    Handed {
        item: ItemId,
        version: VersionId,
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
    /// Short code for this version, unique within its item (see `model::version_codes`).
    pub code: String,
}

/// One save: a version of an item and when it was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Save {
    pub item_id: ItemId,
    pub version_id: VersionId,
    pub at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultStats {
    pub items: u64,
    pub versions: u64,
    pub bytes: u64,
}

/// An item's versions, newest first, each with its short code.
pub(crate) fn version_views(c: &rusqlite::Connection, item: &ItemId) -> Result<Vec<VersionView>> {
    let versions = repo::versions_newest_first(c, item)?;
    let codes = version_codes(&versions.iter().map(|v| v.id.clone()).collect::<Vec<_>>());
    Ok(versions
        .into_iter()
        .zip(codes)
        .map(|(v, code)| VersionView {
            code,
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
    _lock: VaultLock,
    vault: PathBuf,
    vault_key: String,
    inner: Mutex<Inner>,
    /// Canonical keys of items that are tracked and not missing, readable without the writer lock.
    tracked: RwLock<HashSet<String>>,
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
        let (lock, alone) = VaultLock::acquire(vault)?;
        let store = Store::open(&vault.join("folyvo.db"))?;
        let vault_key = canonical_key(vault)?;
        if alone {
            // Nothing else has the vault open, so leftovers are from a crash: a temp file, or an
            // object written before its version row. Another process's in-flight work looks the
            // same, which is why this only runs while the vault is ours alone.
            cas.sweep_tmp()?;
            let known: HashSet<String> = repo::all_object_hashes(store.conn())?.into_iter().collect();
            for hash in cas.object_hashes()? {
                if !known.contains(hash.as_str()) {
                    cas.remove_object(&hash)?;
                }
            }
        }
        lock.share()?;
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let engine = Engine {
            vault: vault.to_path_buf(),
            vault_key,
            inner: Mutex::new(Inner { store, cas }),
            _lock: lock,
            tracked: RwLock::new(HashSet::new()),
            clock,
            ids,
            events,
        };
        {
            let g = engine.lock();
            let items: Vec<ItemId> =
                repo::all_current(g.store.conn())?.into_iter().map(|(i, _, _)| i).collect();
            for item in &items {
                engine.refresh_tracked(&g, item)?;
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

    /// Keep the tracked set in step with one item: present unless the item is missing.
    fn refresh_tracked(&self, inner: &Inner, item: &ItemId) -> Result<()> {
        let c = inner.store.conn();
        let Some(key) = repo::current_key(c, item)? else {
            return Ok(());
        };
        let missing = repo::item_flags(c, item)?.map(|(_, m)| m).unwrap_or(false);
        let mut set = self.tracked.write().unwrap_or_else(|p| p.into_inner());
        if missing {
            set.remove(&key);
        } else {
            set.insert(key);
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
        self.refresh_tracked(inner, &item)?;
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
        self.tracked.write().unwrap_or_else(|p| p.into_inner()).remove(&key);
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
            self.tracked.write().unwrap_or_else(|p| p.into_inner()).remove(&old);
        }
        self.refresh_tracked(inner, item)?;
        self.emit(EngineEvent::ItemUpdated { item: item.clone() });
        Ok(())
    }

    /// Whether Folyvo tracks this path (and the file is not missing). Never waits for the
    /// writer lock, so the watcher can ask about every event.
    pub fn is_tracked(&self, path: &Path) -> bool {
        let Ok(key) = canonical_key(path) else {
            return false;
        };
        self.tracked.read().unwrap_or_else(|p| p.into_inner()).contains(&key)
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
        version_views(c, item)
    }

    pub fn name_version(&self, version: &VersionId, note: Option<&str>) -> Result<()> {
        let g = self.lock();
        if repo::set_note(g.store.conn(), version, note)? {
            Ok(())
        } else {
            Err(CoreError::VersionMissing(version.clone()))
        }
    }

    pub fn export(&self, item: &ItemId, version: &VersionId, dest_dir: &Path) -> Result<ExportResult> {
        let mut g = self.lock();
        let inner = &mut *g;
        let result = Exporter::new(&mut inner.store, &inner.cas, self.clock.as_ref())
            .export(item, version, dest_dir)?;
        self.emit(EngineEvent::Exported { item: item.clone(), destination: result.destination.clone() });
        Ok(result)
    }

    /// A read-only file named like the item that holds `version`, for previews and the share sheet.
    pub fn materialise(&self, item: &ItemId, version: &VersionId) -> Result<PathBuf> {
        let g = self.lock();
        handed::materialise(&g.store, &g.cas, item, version)
    }

    /// Record that `version` was handed to another app (`service`: see `handed::validate_service`).
    pub fn record_handed(&self, item: &ItemId, version: &VersionId, service: &str) -> Result<ShareEvent> {
        let mut g = self.lock();
        let event = handed::record_handed(&mut g.store, self.clock.as_ref(), item, version, service)?;
        self.emit(EngineEvent::Handed { item: item.clone(), version: version.clone() });
        Ok(event)
    }

    /// Copies saved and handed out for `item`, oldest first.
    pub fn events(&self, item: &ItemId) -> Result<Vec<ShareEvent>> {
        let g = self.lock();
        if !repo::item_exists(g.store.conn(), item)? {
            return Err(CoreError::ItemMissing);
        }
        events::share_events(&g.store, item)
    }

    /// Every saved version whose content is exactly the file at `path`, earliest first. The file is
    /// hashed before the writer lock is taken, so a large file does not hold up saving.
    pub fn which_one(&self, path: &Path) -> Result<Vec<which::Match>> {
        let hash = which::hash_of_file(path)?;
        let g = self.lock();
        which::matches_for_hash(&g.store, &hash)
    }

    /// Saves and copies (saved or handed out) with `from_ms <= time < to_ms`, oldest first.
    pub fn activity(&self, from_ms: i64, to_ms: i64) -> Result<(Vec<Save>, Vec<ShareEvent>)> {
        let g = self.lock();
        let c = g.store.conn();
        let saves = repo::saves_between(c, from_ms, to_ms)?
            .into_iter()
            .map(|(item_id, version_id, at)| Save { item_id, version_id, at })
            .collect();
        Ok((saves, repo::events_between(c, from_ms, to_ms)?))
    }

    /// The item whose current location is `path`, missing or not.
    pub fn item_at(&self, path: &Path) -> Result<Option<ItemId>> {
        let Ok(key) = canonical_key(path) else {
            return Ok(None);
        };
        repo::item_at_key(self.lock().store.conn(), &key)
    }

    pub fn restore(&self, item: &ItemId, version: &VersionId) -> Result<Version> {
        let mut g = self.lock();
        let inner = &mut *g;
        let before = repo::latest_version(inner.store.conn(), item)?.map(|v| v.seq).unwrap_or(0);
        let restored = Restorer::new(&mut inner.store, &inner.cas, self.clock.as_ref(), self.ids.as_ref())
            .restore(item, version)?;
        self.refresh_tracked(inner, item)?;
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
    fn activity_returns_saves_and_copies_inside_the_range_only() {
        let mut t = setup();
        let p = t.file("a.docx", b"one");
        t.save(&p, b"one"); // at 2 000
        let v2 = t.save(&p, b"two"); // at 3 000
        let v3 = t.save(&p, b"three"); // at 4 000
        t.clock.advance(500); // 4 500
        t.engine.record_handed(&v2.item_id, &v2.id, "mail").unwrap();
        t.clock.advance(1_500); // 6 000
        t.engine.record_handed(&v2.item_id, &v2.id, "copy").unwrap();
        let (saves, copies) = t.engine.activity(2_500, 5_000).unwrap();
        assert_eq!(
            saves.iter().map(|s| (s.version_id.clone(), s.at)).collect::<Vec<_>>(),
            vec![(v2.id.clone(), 3_000), (v3.id, 4_000)],
            "the save at 2 000 is before the range"
        );
        assert_eq!(saves[0].item_id, v2.item_id);
        assert_eq!(
            copies.iter().map(|c| c.service.clone()).collect::<Vec<_>>(),
            vec![Some("mail".to_string())]
        );
    }

    #[test]
    fn item_at_finds_a_tracked_file_and_nothing_else() {
        let mut t = setup();
        let p = t.file("a.docx", b"one");
        let v = t.save(&p, b"one");
        assert_eq!(t.engine.item_at(&p).unwrap(), Some(v.item_id));
        assert_eq!(t.engine.item_at(&t.file("other.docx", b"x")).unwrap(), None);
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
        assert!(t.engine.is_tracked(&p));
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
    fn unknown_paths_are_not_tracked() {
        let t = setup();
        let p = t.file("never-saved.docx", b"x");
        assert!(!t.engine.is_tracked(&p));
    }

    #[test]
    fn recording_a_handed_copy_emits_an_event() {
        let mut t = setup();
        let p = t.file("a.docx", b"one");
        let v = t.save(&p, b"one");
        t.events();
        let copy = t.engine.materialise(&v.item_id, &v.id).unwrap();
        assert_eq!(fs::read(&copy).unwrap(), b"one");
        let ev = t.engine.record_handed(&v.item_id, &v.id, "airdrop").unwrap();
        assert_eq!(t.events(), vec![EngineEvent::Handed { item: v.item_id.clone(), version: v.id.clone() }]);
        assert_eq!(t.engine.events(&v.item_id).unwrap(), vec![ev]);
        assert!(t.engine.ingest_path(&copy).unwrap().is_none(), "the materialised copy is never ingested");
    }

    #[test]
    fn which_one_recognises_a_copy_through_the_engine() {
        let mut t = setup();
        let p = t.file("a.docx", b"one");
        let v = t.save(&p, b"one");
        let copy = t.dir.path().join("copy.docx");
        fs::write(&copy, b"one").unwrap();
        let m = t.engine.which_one(&copy).unwrap();
        assert_eq!(m.iter().map(|m| m.version.id.clone()).collect::<Vec<_>>(), vec![v.id]);
    }

    #[test]
    fn history_gives_each_version_a_unique_code() {
        let mut t = setup();
        let p = t.file("a.docx", b"one");
        let v1 = t.save(&p, b"one");
        let v2 = t.save(&p, b"two");
        let h = t.engine.history(&v1.item_id).unwrap();
        let suffix = |id: &VersionId| id.as_str().trim_start_matches(VersionId::PREFIX).to_owned();
        assert_eq!(h[0].code, suffix(&v2.id));
        assert_eq!(h[1].code, suffix(&v1.id));
        assert_ne!(h[0].code, h[1].code);
    }

    #[test]
    fn history_is_newest_first_and_shows_names() {
        let mut t = setup();
        let p = t.file("a.docx", b"one");
        let v1 = t.save(&p, b"one");
        let v2 = t.save(&p, b"two");
        t.engine.name_version(&v1.id, Some("Sent to Acme")).unwrap();
        let h = t.engine.history(&v1.item_id).unwrap();
        assert_eq!(h.iter().map(|v| v.seq).collect::<Vec<_>>(), vec![2, 1]);
        assert_eq!(h[0].id, v2.id);
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
        assert!(!t.engine.is_tracked(&p));
        assert!(t.engine.list_recent(5).unwrap()[0].missing);
        t.save(&p, b"back");
        assert!(t.engine.is_tracked(&p));
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
        assert!(!t.engine.is_tracked(&old));
        assert!(t.engine.is_tracked(&new));
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
    fn reopening_the_vault_keeps_history_and_rebuilds_tracking() {
        let mut t = setup();
        let p = t.file("a.docx", b"one");
        let v = t.save(&p, b"one");
        let vault = t.dir.path().join("vault");
        drop(t.engine);
        let again = Engine::open(&vault, t.clock.clone(), Arc::new(SeqIdGen::new())).unwrap();
        assert!(again.is_tracked(&p));
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

    // Final review, Important 1: the CLI and the app share a vault, so one must never sweep away
    // an object the other has written but not yet committed.
    #[test]
    fn an_object_another_open_vault_is_still_writing_is_never_swept() {
        let t = setup();
        let vault = t.dir.path().join("vault");
        let in_flight = crate::model::ContentHash::of_bytes(b"being written by another process");
        let path = t.engine.object_path(&in_flight);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"being written by another process").unwrap();

        let second = Engine::open(&vault, t.clock.clone(), Arc::new(SeqIdGen::new())).unwrap();
        assert!(path.exists(), "the first engine still has the vault open");
        let cli = crate::Vault::open(&vault, t.clock.clone(), Arc::new(SeqIdGen::new())).unwrap();
        drop(t.engine);
        drop(second);
        let third = Engine::open(&vault, t.clock.clone(), Arc::new(SeqIdGen::new())).unwrap();
        assert!(path.exists(), "the command line still has the vault open");
        drop(cli);
        drop(third);
        drop(Engine::open(&vault, t.clock.clone(), Arc::new(SeqIdGen::new())).unwrap());
        assert!(!path.exists(), "alone, the engine sweeps leftovers as before");
    }

    #[test]
    fn a_path_that_cannot_be_resolved_has_no_badge_instead_of_an_error() {
        let t = setup();
        assert!(!t.engine.is_tracked(&t.dir.path().join("gone-folder/a.docx")));
    }

    #[test]
    fn object_path_points_at_the_stored_bytes() {
        let mut t = setup();
        let p = t.file("a.docx", b"stored");
        let v = t.save(&p, b"stored");
        assert_eq!(fs::read(t.engine.object_path(&v.content_hash)).unwrap(), b"stored");
    }
}
