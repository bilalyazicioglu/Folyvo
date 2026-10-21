use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::debounce::{Action, Debouncer, FsEvent};
use crate::engine::Engine;
use crate::error::CoreError;
use crate::ingest::IngestOutcome;
use crate::model::Version;
use crate::ports::Clock;

/// What happened to one path once its quiet period ended.
#[derive(Debug)]
pub enum Applied {
    Saved { path: PathBuf, version: Version },
    Unchanged { path: PathBuf },
    Missing { path: PathBuf },
    Skipped { path: PathBuf },
    Failed { path: PathBuf, reason: String },
}

/// Turns raw file system events into versions: debounce, then ingest or mark missing.
/// Pure of threads and sleeping, so tests drive it with a fake clock. The caller feeds events
/// with `record` and calls `tick` when `next_deadline_in_ms` says something is due.
pub struct WatchLoop {
    engine: Arc<Engine>,
    clock: Arc<dyn Clock>,
    debouncer: Debouncer,
}

impl WatchLoop {
    pub fn new(engine: Arc<Engine>, clock: Arc<dyn Clock>, known_quiet_ms: i64, new_quiet_ms: i64) -> Self {
        Self { engine, clock, debouncer: Debouncer::new(known_quiet_ms, new_quiet_ms) }
    }

    pub fn record(&mut self, path: &Path, event: FsEvent) {
        let known = matches!(self.engine.item_status(path), Ok(Some(_)));
        self.debouncer.record(path, event, self.clock.now_unix_ms(), known);
    }

    pub fn next_deadline_in_ms(&self) -> Option<i64> {
        self.debouncer.next_deadline().map(|at| (at - self.clock.now_unix_ms()).max(0))
    }

    pub fn tick(&mut self) -> Vec<Applied> {
        let due = self.debouncer.due(self.clock.now_unix_ms());
        due.into_iter().map(|d| self.apply(d.path, d.action)).collect()
    }

    fn apply(&self, path: PathBuf, action: Action) -> Applied {
        match action {
            Action::MarkMissing => self.vanished(path),
            Action::Ingest => match self.engine.ingest_path(&path) {
                Ok(Some(IngestOutcome::Created(version))) => Applied::Saved { path, version },
                Ok(Some(IngestOutcome::Unchanged(_))) => Applied::Unchanged { path },
                Ok(None) | Err(CoreError::NotAFile(_)) => Applied::Skipped { path },
                Err(CoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                    self.vanished(path)
                }
                Err(e) => Applied::Failed { path, reason: e.to_string() },
            },
        }
    }

    /// Only a file Folyvo is tracking can go missing. Editors create and delete scratch files all
    /// the time; those were never versioned and must not be reported.
    fn vanished(&self, path: PathBuf) -> Applied {
        if !matches!(self.engine.item_status(&path), Ok(Some(_))) {
            return Applied::Skipped { path };
        }
        match self.engine.mark_missing(&path) {
            Ok(()) => Applied::Missing { path },
            Err(e) => Applied::Failed { path, reason: e.to_string() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{FakeClock, SeqIdGen};
    use std::fs;

    struct Rig {
        dir: tempfile::TempDir,
        clock: Arc<FakeClock>,
        watch: WatchLoop,
    }

    fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("work")).unwrap();
        let clock = Arc::new(FakeClock::new(1_000));
        let engine = Arc::new(
            Engine::open(&dir.path().join("vault"), clock.clone(), Arc::new(SeqIdGen::new())).unwrap(),
        );
        let watch = WatchLoop::new(engine, clock.clone(), 2_000, 5_000);
        Rig { dir, clock, watch }
    }

    impl Rig {
        fn path(&self, name: &str) -> std::path::PathBuf {
            self.dir.path().join("work").join(name)
        }
    }

    #[test]
    fn a_new_file_is_saved_only_after_the_longer_quiet_period() {
        let mut r = rig();
        let f = r.path("a.docx");
        fs::write(&f, b"one").unwrap();
        r.watch.record(&f, FsEvent::Changed);
        r.clock.advance(4_999);
        assert!(r.watch.tick().is_empty());
        r.clock.advance(1);
        let out = r.watch.tick();
        assert!(matches!(out.as_slice(), [Applied::Saved { version, .. }] if version.seq == 1), "{out:?}");
    }

    #[test]
    fn a_known_file_is_saved_after_the_shorter_quiet_period() {
        let mut r = rig();
        let f = r.path("a.docx");
        fs::write(&f, b"one").unwrap();
        r.watch.record(&f, FsEvent::Changed);
        r.clock.advance(5_000);
        r.watch.tick();

        fs::write(&f, b"two").unwrap();
        r.watch.record(&f, FsEvent::Changed);
        r.clock.advance(2_000);
        let out = r.watch.tick();
        assert!(matches!(out.as_slice(), [Applied::Saved { version, .. }] if version.seq == 2), "{out:?}");
    }

    #[test]
    fn a_burst_of_saves_makes_one_version() {
        let mut r = rig();
        let f = r.path("a.docx");
        fs::write(&f, b"one").unwrap();
        r.watch.record(&f, FsEvent::Changed);
        r.clock.advance(5_000);
        r.watch.tick();

        for i in 0..5 {
            fs::write(&f, format!("draft {i}")).unwrap();
            r.watch.record(&f, FsEvent::Changed);
            r.clock.advance(500);
        }
        assert!(r.watch.tick().is_empty());
        r.clock.advance(2_000);
        assert_eq!(r.watch.tick().len(), 1);
    }

    #[test]
    fn saving_unchanged_bytes_reports_unchanged() {
        let mut r = rig();
        let f = r.path("a.docx");
        fs::write(&f, b"one").unwrap();
        r.watch.record(&f, FsEvent::Changed);
        r.clock.advance(5_000);
        r.watch.tick();
        r.watch.record(&f, FsEvent::Changed);
        r.clock.advance(2_000);
        assert!(matches!(r.watch.tick().as_slice(), [Applied::Unchanged { .. }]));
    }

    #[test]
    fn office_owner_files_are_skipped() {
        let mut r = rig();
        let f = r.path("~$a.docx");
        fs::write(&f, b"lock").unwrap();
        r.watch.record(&f, FsEvent::Changed);
        r.clock.advance(5_000);
        assert!(matches!(r.watch.tick().as_slice(), [Applied::Skipped { .. }]));
    }

    #[test]
    fn a_file_that_vanished_before_the_timer_fired_is_marked_missing_not_failed() {
        let mut r = rig();
        let f = r.path("a.docx");
        fs::write(&f, b"one").unwrap();
        r.watch.record(&f, FsEvent::Changed);
        r.clock.advance(5_000);
        r.watch.tick();

        fs::write(&f, b"two").unwrap();
        r.watch.record(&f, FsEvent::Changed);
        fs::remove_file(&f).unwrap();
        r.clock.advance(2_000);
        assert!(matches!(r.watch.tick().as_slice(), [Applied::Missing { .. }]));
    }

    #[test]
    fn a_removal_event_marks_the_file_missing() {
        let mut r = rig();
        let f = r.path("a.docx");
        fs::write(&f, b"one").unwrap();
        r.watch.record(&f, FsEvent::Changed);
        r.clock.advance(5_000);
        r.watch.tick();

        fs::remove_file(&f).unwrap();
        r.watch.record(&f, FsEvent::Removed);
        r.clock.advance(2_000);
        assert!(matches!(r.watch.tick().as_slice(), [Applied::Missing { .. }]));
    }

    #[test]
    fn folders_are_skipped_not_reported_as_failures() {
        let mut r = rig();
        let d = r.path("Report.pages");
        fs::create_dir(&d).unwrap();
        r.watch.record(&d, FsEvent::Changed);
        r.clock.advance(5_000);
        assert!(matches!(r.watch.tick().as_slice(), [Applied::Skipped { .. }]));
    }

    #[test]
    fn next_deadline_tells_the_caller_how_long_it_may_sleep() {
        let mut r = rig();
        assert_eq!(r.watch.next_deadline_in_ms(), None);
        let f = r.path("a.docx");
        fs::write(&f, b"one").unwrap();
        r.watch.record(&f, FsEvent::Changed);
        assert_eq!(r.watch.next_deadline_in_ms(), Some(5_000));
        r.clock.advance(1_200);
        assert_eq!(r.watch.next_deadline_in_ms(), Some(3_800));
    }

    #[test]
    fn a_temp_file_that_appears_and_disappears_is_never_reported() {
        let mut r = rig();
        let f = r.path("scratch.tmp-save");
        fs::write(&f, b"x").unwrap();
        r.watch.record(&f, FsEvent::Changed);
        fs::remove_file(&f).unwrap();
        r.watch.record(&f, FsEvent::Removed);
        r.clock.advance(5_000);
        assert!(matches!(r.watch.tick().as_slice(), [Applied::Skipped { .. }]));

        let g = r.path("ghost.docx");
        fs::write(&g, b"x").unwrap();
        r.watch.record(&g, FsEvent::Changed);
        fs::remove_file(&g).unwrap();
        r.clock.advance(5_000);
        assert!(matches!(r.watch.tick().as_slice(), [Applied::Skipped { .. }]));
    }
}
