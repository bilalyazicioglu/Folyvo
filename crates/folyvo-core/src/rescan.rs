use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use crate::canonical::canonical_key;
use crate::dataless::is_dataless;
use crate::engine::Engine;
use crate::error::{CoreError, Result};
use crate::export::file_hash;
use crate::ignore::{is_generated_folder, is_ignored, is_in_generated_folder, is_package};
use crate::ingest::IngestOutcome;
use crate::reconcile::{match_candidate, Candidate, Known, Match};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RescanReport {
    pub created: usize,
    pub changed: usize,
    pub unchanged: usize,
    pub moved: usize,
    pub marked_missing: usize,
    pub skipped: usize,
    pub dataless: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, String)>,
    /// The scan was asked to stop and ended early. Nothing was marked missing.
    pub stopped: bool,
}

/// How far a scan has come. Sizes tell how long is left better than counts: one big file can take
/// as long as a thousand small ones.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ScanProgress {
    pub done: usize,
    pub total: usize,
    pub done_bytes: u64,
    pub total_bytes: u64,
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>, failed: &mut Vec<(PathBuf, String)>, stop: &AtomicBool) {
    if stop.load(Ordering::SeqCst) {
        return;
    }
    let read = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) => {
            failed.push((dir.to_path_buf(), e.to_string()));
            return;
        }
    };
    let mut entries: Vec<_> = read.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if is_ignored(&path) {
            continue;
        }
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if !is_package(&path) && !is_generated_folder(&path) {
                walk(&path, files, failed, stop);
            }
        } else if kind.is_file() {
            files.push(path);
        }
    }
}

fn mtime_ms(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn rescan_root(engine: &Engine, root: &Path) -> Result<RescanReport> {
    rescan_root_reporting(engine, root, &|_| {})
}

pub fn rescan_root_reporting(
    engine: &Engine,
    root: &Path,
    on_progress: &dyn Fn(ScanProgress),
) -> Result<RescanReport> {
    rescan_root_stoppable(engine, root, on_progress, &AtomicBool::new(false))
}

/// As `rescan_root_reporting`, ending early once `stop` is set, between one file and the next.
pub fn rescan_root_stoppable(
    engine: &Engine,
    root: &Path,
    on_progress: &dyn Fn(ScanProgress),
    stop: &AtomicBool,
) -> Result<RescanReport> {
    if !root.is_dir() {
        return Err(CoreError::io(root, std::io::Error::from(std::io::ErrorKind::NotFound)));
    }
    let mut report = RescanReport::default();
    let mut files = Vec::new();
    walk(root, &mut files, &mut report.failed, stop);
    if stop.load(Ordering::SeqCst) {
        report.stopped = true;
        return Ok(report);
    }

    let tracked = engine.tracked_under(root)?;
    // The walk does not enter generated folders, but a file kept before its folder became one is
    // still kept: missing a save is worse than keeping too much.
    let root_key = canonical_key(root)?;
    files.extend(
        tracked
            .iter()
            .map(|t| &t.path)
            .filter(|path| {
                fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file())
                    && canonical_key(path)
                        .is_ok_and(|key| is_in_generated_folder(Path::new(&root_key), Path::new(&key), path))
            })
            .cloned(),
    );

    let size = |path: &Path| fs::symlink_metadata(path).map(|m| m.len()).unwrap_or(0);
    let total_bytes = files.iter().map(|path| size(path)).sum();
    let mut progress = ScanProgress { total: files.len(), total_bytes, ..ScanProgress::default() };
    on_progress(progress);

    let tracked_keys: std::collections::HashSet<String> =
        tracked.iter().filter_map(|t| canonical_key(&t.path).ok()).collect();

    // A read going on after a pause or a stop does not read again what it already read. Size and
    // time say a file has not changed, as they do for a build; a save that kept both is the price.
    let read_so_far = engine.read_so_far(root)?;
    let already_read: std::collections::HashMap<String, (u64, i64)> = tracked
        .iter()
        .filter(|t| t.latest.seq == 1 && read_so_far.contains(&t.item))
        .filter_map(|t| Some((canonical_key(&t.path).ok()?, (t.latest.size, t.latest.created_at))))
        .collect();

    // Vanished: tracked items whose file is gone. They are the pool for rename matching.
    let mut vanished: Vec<(usize, Known)> = tracked
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            // Only a definite "not found" counts: a permission error must not mark files missing.
            matches!(fs::symlink_metadata(&t.path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        })
        .map(|(i, t)| {
            (
                i,
                Known {
                    item_id: t.item.clone(),
                    inode: None,
                    size: t.latest.size,
                    mtime_ms: 0,
                    hash: t.latest.content_hash.clone(),
                },
            )
        })
        .collect();

    let record = |report: &mut RescanReport, path: &Path, result: Result<Option<IngestOutcome>>| match result
    {
        Ok(Some(IngestOutcome::Created(_))) => report.changed += 1,
        Ok(Some(IngestOutcome::Unchanged(_))) => report.unchanged += 1,
        Ok(None) => report.skipped += 1,
        Err(e) => report.failed.push((path.to_path_buf(), e.to_string())),
    };

    for (done, path) in files.iter().enumerate() {
        if done > 0 {
            progress.done = done;
            on_progress(progress);
        }
        if stop.load(Ordering::SeqCst) {
            report.stopped = true;
            return Ok(report);
        }
        progress.done_bytes += size(path);
        if is_dataless(path) {
            report.dataless.push(path.clone()); // reported, never hashed
            report.skipped += 1;
            continue;
        }
        match engine.is_skipped_by_policy(path) {
            Ok(true) => {
                report.skipped += 1;
                continue;
            }
            Ok(false) => {}
            Err(e) => {
                report.failed.push((path.clone(), e.to_string()));
                continue;
            }
        }
        let key = match canonical_key(path) {
            Ok(k) => k,
            Err(e) => {
                report.failed.push((path.clone(), e.to_string()));
                continue;
            }
        };
        if let Some(&(size, saved_at)) = already_read.get(&key) {
            let same = fs::symlink_metadata(path).is_ok_and(|m| m.len() == size && mtime_ms(&m) == saved_at);
            if same {
                report.unchanged += 1;
                continue;
            }
        }
        if tracked_keys.contains(&key) {
            let result = engine.ingest_found(path);
            record(&mut report, path, result);
            continue;
        }

        // Unknown path: is it a vanished item that moved?
        let meta = match fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                report.failed.push((path.clone(), e.to_string()));
                continue;
            }
        };
        let pool: Vec<Known> = vanished.iter().map(|(_, k)| k.clone()).collect();
        let candidate = Candidate { inode: None, size: meta.len(), mtime_ms: mtime_ms(&meta) };
        let decision = match_candidate(&candidate, &pool, &mut || file_hash(path));
        match decision {
            Ok(Match::Same(item)) => match engine.move_item(&item, path) {
                Ok(()) => {
                    vanished.retain(|(_, k)| k.item_id != item);
                    report.moved += 1;
                    let result = engine.ingest_found(path);
                    if matches!(result, Ok(Some(IngestOutcome::Created(_)))) {
                        report.changed += 1;
                    }
                }
                Err(e) => report.failed.push((path.clone(), e.to_string())),
            },
            Ok(Match::NewItem) | Ok(Match::Ambiguous) => match engine.ingest_found(path) {
                Ok(Some(IngestOutcome::Created(_))) => report.created += 1,
                Ok(Some(IngestOutcome::Unchanged(_))) => report.unchanged += 1,
                Ok(None) => report.skipped += 1,
                Err(e) => report.failed.push((path.clone(), e.to_string())),
            },
            Err(e) => report.failed.push((path.clone(), e.to_string())),
        }
    }

    progress.done = files.len();
    progress.done_bytes = total_bytes;
    on_progress(progress);

    for (index, _) in vanished {
        let t = &tracked[index];
        if t.missing {
            continue;
        }
        match engine.mark_item_missing(&t.item) {
            Ok(()) => report.marked_missing += 1,
            Err(e) => report.failed.push((t.path.clone(), e.to_string())),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Engine;
    use crate::ports::{FakeClock, SeqIdGen};
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};
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
        fn root(&self) -> PathBuf {
            fs::canonicalize(self.dir.path()).unwrap().join("work")
        }
        fn put(&self, rel: &str, bytes: &[u8]) -> PathBuf {
            let p = self.root().join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, bytes).unwrap();
            p
        }
        fn rescan(&self) -> RescanReport {
            self.clock.advance(1_000);
            rescan_root(&self.engine, &self.root()).unwrap()
        }
    }

    #[test]
    fn a_first_scan_versions_nested_files_and_skips_junk_symlinks_and_packages() {
        let t = setup();
        t.put("a.docx", b"a");
        t.put("sub/deeper/b.xlsx", b"b");
        t.put(".DS_Store", b"junk");
        t.put("~$a.docx", b"lock");
        t.put(".hidden/secret.docx", b"hidden");
        t.put("Deck.key/Index/slide.xml", b"package guts");
        #[cfg(unix)]
        std::os::unix::fs::symlink(t.root().join("a.docx"), t.root().join("alias.docx")).unwrap();
        let r = t.rescan();
        assert_eq!(r.created, 2, "{r:?}");
        assert_eq!(t.engine.vault_stats().unwrap().items, 2);
    }

    #[test]
    fn scanning_twice_changes_nothing() {
        let t = setup();
        t.put("a.docx", b"a");
        t.put("b.docx", b"b");
        t.rescan();
        let r = t.rescan();
        assert_eq!((r.created, r.changed, r.moved, r.marked_missing), (0, 0, 0, 0), "{r:?}");
        assert_eq!(r.unchanged, 2);
        assert_eq!(t.engine.vault_stats().unwrap().versions, 2);
    }

    #[test]
    fn a_file_edited_while_the_app_was_closed_gets_a_new_version() {
        let t = setup();
        let p = t.put("a.docx", b"one");
        t.rescan();
        fs::write(&p, b"two").unwrap();
        let r = t.rescan();
        assert_eq!(r.changed, 1, "{r:?}");
        assert_eq!(t.engine.vault_stats().unwrap().versions, 2);
    }

    #[test]
    fn a_file_renamed_while_the_app_was_closed_keeps_its_item_and_history() {
        let t = setup();
        let old = t.put("draft.docx", b"one");
        t.rescan();
        let item = t.engine.list_recent(1).unwrap()[0].item_id.clone();
        fs::rename(&old, t.root().join("final.docx")).unwrap();
        let r = t.rescan();
        assert_eq!(r.moved, 1, "{r:?}");
        assert_eq!((r.created, r.marked_missing), (0, 0));
        let stats = t.engine.vault_stats().unwrap();
        assert_eq!((stats.items, stats.versions), (1, 1));
        assert_eq!(t.engine.history(&item).unwrap().len(), 1);
        assert!(t.engine.is_tracked(&t.root().join("final.docx")));
    }

    #[test]
    fn moving_a_file_into_a_subfolder_is_a_move() {
        let t = setup();
        let old = t.put("a.docx", b"payload");
        t.rescan();
        fs::create_dir(t.root().join("archive")).unwrap();
        fs::rename(&old, t.root().join("archive/a.docx")).unwrap();
        assert_eq!(t.rescan().moved, 1);
        assert_eq!(t.engine.vault_stats().unwrap().items, 1);
    }

    #[test]
    fn a_deleted_file_is_marked_missing_once() {
        let t = setup();
        let p = t.put("a.docx", b"one");
        t.rescan();
        fs::remove_file(&p).unwrap();
        assert_eq!(t.rescan().marked_missing, 1);
        assert_eq!(t.rescan().marked_missing, 0, "already flagged, no repeat event");
        assert!(t.engine.list_recent(1).unwrap()[0].missing);
    }

    // Review Focus 6: a whole client folder deleted or reorganised while the app was closed.
    #[test]
    fn deleting_a_whole_subfolder_does_not_abort_the_scan() {
        let t = setup();
        t.put("keep.docx", b"keep");
        t.put("client/deep/a.docx", b"a");
        t.put("client/b.docx", b"b");
        t.rescan();
        fs::remove_dir_all(t.root().join("client")).unwrap();
        let r = t.rescan();
        assert_eq!(r.marked_missing, 2, "{r:?}");
        assert!(r.failed.is_empty(), "{r:?}");
        assert_eq!(r.unchanged, 1);
        let again = t.rescan();
        assert_eq!(again.marked_missing, 0);
    }

    #[test]
    fn identical_files_renamed_together_are_never_merged() {
        let t = setup();
        let (a, b) = (t.put("a.docx", b"same"), t.put("b.docx", b"same"));
        t.rescan();
        fs::rename(&a, t.root().join("c.docx")).unwrap();
        fs::rename(&b, t.root().join("d.docx")).unwrap();
        let r = t.rescan();
        assert_eq!(r.moved, 0, "{r:?}");
        assert_eq!(r.created, 2);
        assert_eq!(r.marked_missing, 2);
        assert_eq!(t.engine.vault_stats().unwrap().items, 4);
    }

    #[test]
    fn files_that_cannot_be_read_are_reported_not_hidden_and_do_not_stop_the_scan() {
        let t = setup();
        t.put("good.docx", b"good");
        let bad_dir = t.root().join("locked");
        fs::create_dir(&bad_dir).unwrap();
        fs::write(bad_dir.join("x.docx"), b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&bad_dir, fs::Permissions::from_mode(0o000)).unwrap();
            let r = t.rescan();
            fs::set_permissions(&bad_dir, fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(r.created, 1, "{r:?}");
            assert_eq!(r.failed.len(), 1, "{r:?}");
            assert!(r.failed[0].0.ends_with("locked"));
        }
    }

    #[test]
    fn an_export_copy_is_never_mistaken_for_the_original_that_moved() {
        let t = setup();
        let orig = t.put("a.docx", b"payload");
        t.rescan();
        let item = t.engine.list_recent(1).unwrap()[0].item_id.clone();
        let ver = t.engine.history(&item).unwrap()[0].id.clone();
        let out = t.root().join("out");
        fs::create_dir(&out).unwrap();
        let copy = t.engine.export(&item, &ver, &out).unwrap().destination;
        fs::remove_file(&orig).unwrap();
        let r = t.rescan();
        assert_eq!(r.moved, 0, "{r:?}");
        assert_eq!(r.marked_missing, 1, "{r:?}");
        assert!(copy.exists());
        assert!(!t.engine.is_tracked(&copy));
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_folder_does_not_make_its_files_look_deleted() {
        use std::os::unix::fs::PermissionsExt;
        let t = setup();
        t.put("locked/a.docx", b"a");
        t.rescan();
        let dir = t.root().join("locked");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o000)).unwrap();
        let r = t.rescan();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(r.marked_missing, 0, "{r:?}");
        assert!(!t.engine.list_recent(1).unwrap()[0].missing);
    }

    #[test]
    fn export_destinations_inside_the_root_are_left_alone() {
        let t = setup();
        let p = t.put("a.docx", b"payload");
        t.rescan();
        let item = t.engine.list_recent(1).unwrap()[0].item_id.clone();
        let ver = t.engine.history(&item).unwrap()[0].id.clone();
        let out = t.root().join("out");
        fs::create_dir(&out).unwrap();
        t.engine.export(&item, &ver, &out).unwrap();
        let r = t.rescan();
        assert_eq!(r.created, 0, "{r:?}");
        assert_eq!(t.engine.vault_stats().unwrap().items, 1);
        assert!(p.exists());
    }

    const CACHE_TAG: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55\n# Made by a build tool.\n";

    #[test]
    fn the_scan_and_the_watcher_both_skip_generated_folders() {
        let t = setup();
        t.engine.add_root(&t.root()).unwrap();
        let kept = t.put("report.docx", b"words");
        t.put("target/CACHEDIR.TAG", CACHE_TAG);
        let skipped = [
            t.put("app/node_modules/pkg/index.js", b"a"),
            t.put("web/Node_Modules/index.js", b"b"),
            t.put("target/debug/main.o", b"c"),
            t.put("other/Cache/deep/er/blob", b"d"),
            t.root().join("target/CACHEDIR.TAG"),
        ];
        t.put("other/Cache/CACHEDIR.TAG", CACHE_TAG);

        let r = t.rescan();

        assert_eq!(r.created, 1, "{r:?}");
        assert!(t.engine.is_tracked(&kept));
        for path in &skipped {
            assert!(!t.engine.is_tracked(path), "the scan kept {path:?}");
            assert!(t.engine.ingest_path(path).unwrap().is_none(), "the watcher kept {path:?}");
        }
        assert_eq!(t.engine.vault_stats().unwrap().items, 1);
    }

    #[test]
    fn folders_that_only_look_generated_are_kept_by_the_scan_and_the_watcher() {
        let t = setup();
        t.engine.add_root(&t.root()).unwrap();
        t.put("mine/CACHEDIR.TAG", b"I named a note this.");
        t.put("short/CACHEDIR.TAG", b"Signature: 8a47");
        let kept = [
            t.put("target/debug/main.o", b"a"),
            t.put("build/chapter.docx", b"b"),
            t.put("dist/flyer.pdf", b"c"),
            t.put("mine/plan.docx", b"d"),
            t.put("short/plan.docx", b"e"),
            t.put("notes/node_modules", b"a file, not a folder"),
            t.put("node_modules.docx", b"f"),
        ];

        t.rescan();

        for path in &kept {
            assert!(t.engine.is_tracked(path), "the scan skipped {path:?}");
        }
        let other = t.put("target/debug/later.o", b"g");
        assert!(t.engine.ingest_path(&other).unwrap().is_some(), "the watcher skipped {other:?}");
    }

    #[test]
    fn a_folder_chosen_directly_is_kept_even_when_it_is_generated() {
        let t = setup();
        let tagged = t.root().join("target");
        t.put("target/CACHEDIR.TAG", CACHE_TAG);
        let object = t.put("target/debug/main.o", b"a");
        let modules = t.root().join("node_modules");
        let script = t.put("node_modules/index.js", b"b");

        for (root, file) in [(&tagged, &object), (&modules, &script)] {
            t.engine.add_root(root).unwrap();
            let r = rescan_root(&t.engine, root).unwrap();
            assert!(r.created >= 1, "{r:?}");
            assert!(t.engine.is_tracked(file), "the scan skipped {file:?}");
            assert!(t.engine.ingest_path(file).unwrap().is_some(), "the watcher skipped {file:?}");
            t.engine.remove_root(root).unwrap();
        }
    }

    #[test]
    fn a_generated_folder_chosen_inside_another_kept_folder_is_kept() {
        let t = setup();
        t.engine.add_root(&t.root()).unwrap();
        let tagged = t.root().join("target");
        t.put("target/CACHEDIR.TAG", CACHE_TAG);
        let object = t.put("target/debug/main.o", b"a");
        t.engine.add_root(&tagged).unwrap();

        let r = rescan_root(&t.engine, &tagged).unwrap();

        assert!(r.created >= 1, "{r:?}");
        assert!(t.engine.is_tracked(&object));
        assert!(t.engine.ingest_path(&object).unwrap().is_some());
    }

    #[test]
    fn a_file_kept_before_its_folder_became_generated_keeps_getting_versions() {
        let t = setup();
        t.engine.add_root(&t.root()).unwrap();
        let p = t.put("build/main.docx", b"one");
        t.rescan();
        t.put("build/CACHEDIR.TAG", CACHE_TAG);
        let new_file = t.put("build/new.o", b"made later");

        fs::write(&p, b"two").unwrap();
        let r = t.rescan();

        assert_eq!((r.created, r.changed, r.marked_missing), (0, 1, 0), "{r:?}");
        assert!(!t.engine.is_tracked(&new_file), "a new file in a generated folder is still skipped");
        fs::write(&p, b"three").unwrap();
        assert!(matches!(t.engine.ingest_path(&p).unwrap(), Some(IngestOutcome::Created(_))));
        assert!(t.engine.ingest_path(&new_file).unwrap().is_none());
        assert_eq!(t.engine.vault_stats().unwrap().versions, 3);
    }

    #[test]
    fn a_kept_file_in_a_hidden_folder_is_not_brought_back_by_the_scan() {
        let t = setup();
        let p = t.put(".hidden/a.docx", b"one");
        t.engine.ingest_path(&p).unwrap();
        t.engine.add_root(&t.root()).unwrap();

        fs::write(&p, b"two").unwrap();
        let r = t.rescan();

        assert_eq!((r.changed, r.unchanged, r.skipped), (0, 0, 0), "{r:?}");
    }

    fn stopped_after(t: &T, files_read: usize) -> RescanReport {
        let stop = AtomicBool::new(false);
        let on_progress = |p: ScanProgress| {
            if p.done >= files_read {
                stop.store(true, Ordering::SeqCst);
            }
        };
        rescan_root_stoppable(&t.engine, &t.root(), &on_progress, &stop).unwrap()
    }

    #[test]
    fn a_stopped_scan_ends_early() {
        let t = setup();
        for name in ["a.txt", "b.txt", "c.txt", "d.txt"] {
            t.put(name, name.as_bytes());
        }
        let r = stopped_after(&t, 2);
        assert!(r.stopped);
        assert_eq!(t.engine.vault_stats().unwrap().items, 2);
    }

    #[test]
    fn a_stopped_scan_marks_nothing_missing() {
        let t = setup();
        let gone = t.put("a.txt", b"one");
        t.put("b.txt", b"two");
        t.put("c.txt", b"three");
        t.rescan();
        fs::remove_file(&gone).unwrap();
        let r = stopped_after(&t, 1);
        assert!(r.stopped);
        assert_eq!(r.marked_missing, 0, "{r:?}");
    }

    #[test]
    fn a_scan_stopped_before_it_starts_keeps_nothing() {
        let t = setup();
        t.put("sub/a.txt", b"one");
        let stop = AtomicBool::new(true);
        let r = rescan_root_stoppable(&t.engine, &t.root(), &|_| {}, &stop).unwrap();
        assert!(r.stopped);
        assert_eq!(t.engine.vault_stats().unwrap().items, 0);
    }

    #[test]
    fn forgetting_a_stopped_read_leaves_the_vault_as_it_was() {
        let t = setup();
        let kept = t.put("kept.txt", b"kept");
        t.rescan();
        let before = t.engine.vault_stats().unwrap();
        t.clock.advance(1_000);
        t.engine.begin_read(&t.root()).unwrap();
        let same_bytes = t.put("copy-of-kept.txt", b"kept");
        let new = t.put("new.txt", b"new");
        t.rescan();
        let new_hash = crate::model::ContentHash::of_bytes(b"new");
        assert!(t.engine.object_path(&new_hash).exists());

        t.engine.forget_read(&t.root()).unwrap();

        assert_eq!(t.engine.vault_stats().unwrap(), before);
        assert!(!t.engine.is_tracked(&new));
        assert!(!t.engine.is_tracked(&same_bytes));
        assert!(t.engine.is_tracked(&kept));
        assert!(!t.engine.object_path(&new_hash).exists(), "nothing else uses these bytes");
        let shared = crate::model::ContentHash::of_bytes(b"kept");
        assert!(t.engine.object_path(&shared).exists(), "the kept file still uses these bytes");
        assert_eq!(fs::read(&new).unwrap(), b"new", "the person's own file is never touched");
    }

    #[test]
    fn forgetting_spares_a_file_saved_again_since() {
        let t = setup();
        t.engine.begin_read(&t.root()).unwrap();
        let p = t.put("a.txt", b"one");
        t.rescan();
        fs::write(&p, b"two").unwrap();
        t.rescan();
        t.engine.forget_read(&t.root()).unwrap();
        assert!(t.engine.is_tracked(&p));
        assert_eq!(t.engine.vault_stats().unwrap().versions, 2);
    }

    #[test]
    fn forgetting_a_finished_read_takes_nothing_back() {
        let t = setup();
        t.engine.begin_read(&t.root()).unwrap();
        let p = t.put("a.txt", b"one");
        t.rescan();
        t.engine.finish_read(&t.root()).unwrap();
        t.engine.forget_read(&t.root()).unwrap();
        assert!(t.engine.is_tracked(&p));
    }

    #[test]
    fn forgetting_leaves_a_folder_whose_name_only_starts_the_same() {
        let t = setup();
        t.engine.begin_read(&t.root()).unwrap();
        let sibling =
            t.root().with_file_name(format!("{}-2", t.root().file_name().unwrap().to_string_lossy()));
        fs::create_dir(&sibling).unwrap();
        let p = sibling.join("a.txt");
        fs::write(&p, b"one").unwrap();
        rescan_root(&t.engine, &sibling).unwrap();
        let inside = t.put("b.txt", b"two");
        t.rescan();
        t.engine.forget_read(&t.root()).unwrap();
        assert!(t.engine.is_tracked(&p));
        assert!(!t.engine.is_tracked(&inside));
    }

    /// A file kept by a read that has not ended, last saved at a time the clock has passed.
    fn read_a_file(t: &T) -> PathBuf {
        t.clock.advance(1_000_000);
        t.engine.begin_read(&t.root()).unwrap();
        let p = t.put("a.txt", b"one");
        saved_at(&p, 100);
        t.rescan();
        assert_eq!(t.engine.vault_stats().unwrap().versions, 1);
        p
    }

    #[test]
    fn going_on_with_a_read_passes_over_what_it_already_read() {
        let t = setup();
        let p = read_a_file(&t);
        // The same size and time: only a scan that opens the file can tell the difference.
        fs::write(&p, b"two").unwrap();
        saved_at(&p, 100);
        let r = t.rescan();
        assert_eq!(r.unchanged, 1, "{r:?}");
        assert_eq!(t.engine.vault_stats().unwrap().versions, 1);
    }

    #[test]
    fn going_on_with_a_read_keeps_a_file_changed_since() {
        let t = setup();
        let p = read_a_file(&t);
        fs::write(&p, b"two").unwrap();
        saved_at(&p, 200);
        t.rescan();
        assert_eq!(t.engine.vault_stats().unwrap().versions, 2);
    }

    #[test]
    fn a_finished_read_passes_over_nothing() {
        let t = setup();
        let p = read_a_file(&t);
        t.engine.finish_read(&t.root()).unwrap();
        fs::write(&p, b"two").unwrap();
        saved_at(&p, 100);
        t.rescan();
        assert_eq!(t.engine.vault_stats().unwrap().versions, 2);
    }

    #[test]
    fn a_root_that_does_not_exist_is_an_error() {
        let t = setup();
        assert!(rescan_root(&t.engine, &t.dir.path().join("nope")).is_err());
    }

    fn saved_at(path: &std::path::Path, secs: u64) {
        let when = UNIX_EPOCH + std::time::Duration::from_secs(secs);
        fs::File::options().write(true).open(path).unwrap().set_modified(when).unwrap();
    }

    // Reading a folder is not saving: a file last saved in March is not a save made today.
    #[test]
    fn a_file_found_by_a_scan_is_dated_when_it_was_last_saved() {
        let t = setup();
        t.clock.advance(10_000_000);
        let root = t.dir.path().join("w");
        fs::create_dir(&root).unwrap();
        let file = root.join("old.docx");
        fs::write(&file, b"one").unwrap();
        saved_at(&file, 5_000);
        rescan_root(&t.engine, &root).unwrap();
        let saves = t.engine.activity(0, i64::MAX).unwrap().0;
        assert_eq!(saves[0].at, 5_000_000);
    }

    #[test]
    fn a_scan_never_dates_a_save_before_the_one_it_follows_or_after_now() {
        let t = setup();
        t.clock.advance(10_000_000);
        let root = t.dir.path().join("w");
        fs::create_dir(&root).unwrap();
        let file = root.join("a.docx");
        fs::write(&file, b"one").unwrap();
        saved_at(&file, 5_000);
        rescan_root(&t.engine, &root).unwrap();
        fs::write(&file, b"two").unwrap();
        saved_at(&file, 1_000);
        rescan_root(&t.engine, &root).unwrap();
        fs::write(&file, b"three").unwrap();
        saved_at(&file, 999_999);
        rescan_root(&t.engine, &root).unwrap();
        let saves = t.engine.activity(0, i64::MAX).unwrap().0;
        let dates: Vec<i64> = saves.iter().map(|s| s.at).collect();
        let now = 10_000_000 + 1_000;
        assert!(dates[0] == 5_000_000 && dates[1] == 5_000_000 && dates[2] <= now, "{dates:?}");
    }

    #[test]
    fn a_scan_reports_how_far_it_has_come_and_ends_complete() {
        let t = setup();
        let root = t.dir.path().join("work");
        fs::write(root.join("a.docx"), b"12345").unwrap();
        fs::write(root.join("b.docx"), b"123").unwrap();
        fs::write(root.join("c.docx"), b"12").unwrap();
        let seen = std::cell::RefCell::new(Vec::new());
        rescan_root_reporting(&t.engine, &root, &|p| seen.borrow_mut().push(p)).unwrap();
        let seen = seen.into_inner();
        assert!(seen.iter().all(|p| p.total == 3 && p.total_bytes == 10), "{seen:?}");
        assert!(seen.windows(2).all(|w| w[0].done <= w[1].done), "{seen:?}");
        let last = *seen.last().unwrap();
        assert_eq!((last.done, last.done_bytes), (3, 10));
    }
}
