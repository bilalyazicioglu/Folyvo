use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::canonical::canonical_key;
use crate::dataless::is_dataless;
use crate::engine::Engine;
use crate::error::{CoreError, Result};
use crate::export::file_hash;
use crate::ignore::{is_ignored, is_package};
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
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>, failed: &mut Vec<(PathBuf, String)>) {
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
            if !is_package(&path) {
                walk(&path, files, failed);
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
    if !root.is_dir() {
        return Err(CoreError::io(root, std::io::Error::from(std::io::ErrorKind::NotFound)));
    }
    let mut report = RescanReport::default();
    let mut files = Vec::new();
    walk(root, &mut files, &mut report.failed);

    let tracked = engine.tracked_under(root)?;
    let tracked_keys: std::collections::HashSet<String> =
        tracked.iter().filter_map(|t| canonical_key(&t.path).ok()).collect();

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

    for path in &files {
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
        if tracked_keys.contains(&key) {
            let result = engine.ingest_path(path);
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
                    let result = engine.ingest_path(path);
                    if matches!(result, Ok(Some(IngestOutcome::Created(_)))) {
                        report.changed += 1;
                    }
                }
                Err(e) => report.failed.push((path.clone(), e.to_string())),
            },
            Ok(Match::NewItem) | Ok(Match::Ambiguous) => match engine.ingest_path(path) {
                Ok(Some(IngestOutcome::Created(_))) => report.created += 1,
                Ok(Some(IngestOutcome::Unchanged(_))) => report.unchanged += 1,
                Ok(None) => report.skipped += 1,
                Err(e) => report.failed.push((path.clone(), e.to_string())),
            },
            Err(e) => report.failed.push((path.clone(), e.to_string())),
        }
    }

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
            self.dir.path().join("work")
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
        assert!(t.engine.item_status(&t.root().join("final.docx")).unwrap().is_some());
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
        assert_eq!(t.engine.item_status(&copy).unwrap(), None);
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

    #[test]
    fn a_root_that_does_not_exist_is_an_error() {
        let t = setup();
        assert!(rescan_root(&t.engine, &t.dir.path().join("nope")).is_err());
    }
}
