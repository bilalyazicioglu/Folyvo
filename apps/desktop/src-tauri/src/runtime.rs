use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use folyvo_core::engine::{Engine, RecvError};
use folyvo_core::ports::{Clock, IdGen};
use folyvo_core::rescan::ScanProgress;
use folyvo_core::runner::{OnApplied, Runner};
use folyvo_core::watch::Applied;
use folyvo_core::CoreError;

use crate::api::AppApi;
use crate::changes::ChangeWorker;
use crate::dto::{EngineEventDto, FolderDto, ReadingDto};
use crate::errors::UserError;

/// Receives every event the UI should hear about. The Tauri layer forwards them to the webviews.
pub trait Notifier: Send + Sync {
    fn notify(&self, event: EngineEventDto);
}

/// How often a read in progress tells the UI how far it has come.
const PROGRESS_EVERY: Duration = Duration::from_millis(500);

/// Too little has been read before this to say how long the rest will take.
const ESTIMATE_AFTER_MS: i64 = 3_000;

/// How long the rest takes at the rate so far. The rate is uneven both ways: big files fill the
/// bytes quickly while thousands of small ones take long for their size, so the longer answer wins.
fn seconds_left(elapsed_ms: i64, p: ScanProgress) -> Option<u32> {
    if elapsed_ms < ESTIMATE_AFTER_MS || p.done == 0 || p.done_bytes == 0 {
        return None;
    }
    let at_rate = |done: u64, total: u64| {
        u128::from(total.saturating_sub(done)) * elapsed_ms as u128 / u128::from(done)
    };
    let by_files = at_rate(p.done as u64, p.total as u64);
    let by_bytes = at_rate(p.done_bytes, p.total_bytes);
    Some((by_files.max(by_bytes) / 1_000) as u32)
}

struct Reading {
    started_ms: i64,
    progress: ScanProgress,
}

impl Reading {
    fn dto(&self, now_ms: i64) -> ReadingDto {
        let p = self.progress;
        ReadingDto {
            done: p.done as u32,
            total: p.total as u32,
            seconds_left: seconds_left(now_ms - self.started_ms, p),
        }
    }
}

pub struct AppRuntime {
    engine: Arc<Engine>,
    clock: Arc<dyn Clock>,
    quiet: (i64, i64),
    runners: Mutex<HashMap<PathBuf, Runner>>,
    problems: Mutex<HashMap<PathBuf, UserError>>,
    /// Folders whose first read has not ended. Reading one can take minutes.
    reading: Mutex<HashMap<PathBuf, Reading>>,
    notifier: Arc<dyn Notifier>,
    changes: ChangeWorker,
    missed: AtomicUsize,
}

impl AppRuntime {
    pub fn start(
        vault: &Path,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGen>,
        quiet: (i64, i64),
        notifier: Arc<dyn Notifier>,
    ) -> Result<Arc<AppRuntime>, UserError> {
        let engine = Arc::new(Engine::open(vault, clock.clone(), ids)?);

        let mut events = engine.subscribe();
        let pump = notifier.clone();
        thread::spawn(move || loop {
            match events.blocking_recv() {
                Ok(event) => pump.notify(event.into()),
                Err(RecvError::Lagged(_)) => pump.notify(EngineEventDto::Resync),
                Err(RecvError::Closed) => break,
            }
        });

        let runtime = Arc::new(AppRuntime {
            changes: ChangeWorker::start(engine.clone()),
            engine,
            clock,
            quiet,
            runners: Mutex::new(HashMap::new()),
            problems: Mutex::new(HashMap::new()),
            reading: Mutex::new(HashMap::new()),
            notifier,
            missed: AtomicUsize::new(0),
        });
        for root in runtime.engine.roots()? {
            match runtime.start_runner(&root, &|_| {}) {
                Ok(changed) => {
                    runtime.missed.fetch_add(changed, Ordering::SeqCst);
                }
                Err(problem) => {
                    runtime.problems.lock().unwrap().insert(root, problem);
                }
            }
        }
        Ok(runtime)
    }

    pub fn api(&self) -> AppApi {
        AppApi::new(self.engine.clone())
    }

    /// Asks for the saves of `item` that are not compared yet to be compared first.
    pub fn backfill(&self, item: &str) {
        if let Ok(item) = folyvo_core::model::ItemId::parse(item) {
            self.changes.backfill(&self.engine, &item);
        }
    }

    /// How many kept files changed while Folyvo was not running: the saves between their last kept
    /// version and how they look now are gone. A new file is not counted; nothing of it was kept before.
    pub fn missed_while_closed(&self) -> usize {
        self.missed.load(Ordering::SeqCst)
    }

    pub fn dismiss_missed(&self) {
        self.missed.store(0, Ordering::SeqCst);
    }

    /// Remembers the folder and returns; the first read goes on behind it and can take minutes.
    /// `folders()` shows how it is going, and a read that fails shows on the folder's row.
    pub fn add_folder(self: &Arc<Self>, folder: &Path) -> Result<(), UserError> {
        self.engine.add_root(folder)?;
        let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
        self.problems.lock().unwrap().remove(folder);
        self.problems.lock().unwrap().remove(&canonical);
        if self.runners.lock().unwrap().contains_key(&canonical) {
            return Ok(());
        }
        {
            let mut reading = self.reading.lock().unwrap();
            if reading.contains_key(&canonical) {
                return Ok(());
            }
            let started_ms = self.clock.now_unix_ms();
            reading.insert(canonical.clone(), Reading { started_ms, progress: ScanProgress::default() });
        }
        self.notifier.notify(EngineEventDto::RootsChanged);
        let rt = self.clone();
        thread::spawn(move || {
            let told = Mutex::new(Instant::now());
            let on_progress = |progress: ScanProgress| {
                if let Some(reading) = rt.reading.lock().unwrap().get_mut(&canonical) {
                    reading.progress = progress;
                }
                let mut told = told.lock().unwrap();
                if told.elapsed() >= PROGRESS_EVERY {
                    *told = Instant::now();
                    rt.notifier.notify(EngineEventDto::RootsChanged);
                }
            };
            if let Err(problem) = rt.start_runner(&canonical, &on_progress) {
                rt.problems.lock().unwrap().insert(canonical.clone(), problem);
            }
            rt.reading.lock().unwrap().remove(&canonical);
            rt.notifier.notify(EngineEventDto::RootsChanged);
        });
        Ok(())
    }

    /// A read cannot be stopped halfway, so a folder being read stays until the read ends.
    pub fn remove_folder(&self, folder: &Path) -> Result<(), UserError> {
        let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
        if self.reading.lock().unwrap().contains_key(&canonical) {
            return Err(UserError::new(
                "folder_busy",
                "Folyvo is still reading that folder",
                "Try again when it has finished.",
            ));
        }
        let runner = self.runners.lock().unwrap().remove(&canonical);
        drop(runner); // stops the watcher thread
        self.problems.lock().unwrap().remove(folder);
        self.problems.lock().unwrap().remove(&canonical);
        self.engine.remove_root(folder)?;
        Ok(())
    }

    pub fn folders(&self) -> Vec<FolderDto> {
        let problems = self.problems.lock().unwrap();
        let reading = self.reading.lock().unwrap();
        let now = self.clock.now_unix_ms();
        self.engine
            .roots()
            .unwrap_or_default()
            .into_iter()
            .map(|root| FolderDto {
                problem: problems.get(&root).cloned(),
                reading: reading.get(&root).map(|r| r.dto(now)),
                path: root.to_string_lossy().into_owned(),
            })
            .collect()
    }

    /// Returns how many kept files the first scan found changed.
    fn start_runner(&self, folder: &Path, on_progress: &dyn Fn(ScanProgress)) -> Result<usize, UserError> {
        let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
        if self.runners.lock().unwrap().contains_key(&canonical) {
            return Ok(0);
        }
        let on_applied: OnApplied = Arc::new(|applied| {
            if let Applied::Failed { path, reason } = applied {
                eprintln!("folyvo: could not save {}: {reason}", path.display());
            }
        });
        let (runner, report) = Runner::start_reporting(
            self.engine.clone(),
            self.clock.clone(),
            folder,
            self.quiet,
            on_applied,
            on_progress,
        )?;
        // Only the folder itself: one unreadable folder inside it must not stop the rest being kept.
        if report.failed.iter().any(|(path, _)| path == runner.folder()) {
            let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
            return Err(CoreError::io(runner.folder(), denied).into());
        }
        self.runners.lock().unwrap().insert(runner.folder().to_path_buf(), runner);
        Ok(report.changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use folyvo_core::ports::{RandomIdGen, SystemClock};
    use std::fs;
    use std::sync::mpsc;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    struct Collect(Mutex<mpsc::Sender<EngineEventDto>>);
    impl Notifier for Collect {
        fn notify(&self, event: EngineEventDto) {
            let _ = self.0.lock().unwrap().send(event);
        }
    }

    fn start(dir: &std::path::Path) -> (Arc<AppRuntime>, mpsc::Receiver<EngineEventDto>) {
        let (tx, rx) = mpsc::channel();
        let rt = AppRuntime::start(
            &dir.join("vault"),
            Arc::new(SystemClock),
            Arc::new(RandomIdGen),
            (100, 100),
            Arc::new(Collect(Mutex::new(tx))),
        )
        .unwrap();
        (rt, rx)
    }

    fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if f() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("timed out waiting for {what}");
    }

    fn add_and_read(rt: &Arc<AppRuntime>, folder: &std::path::Path) {
        rt.add_folder(folder).unwrap();
        wait_until("the folder to be read", || rt.folders().iter().all(|f| f.reading.is_none()));
    }

    fn recent_names(rt: &AppRuntime) -> Vec<String> {
        rt.api().recent(50).unwrap().into_iter().map(|i| i.name).collect()
    }

    #[test]
    fn a_file_saved_in_an_added_folder_becomes_a_version_and_an_event() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Client work");
        fs::create_dir(&folder).unwrap();
        let (rt, rx) = start(dir.path());
        add_and_read(&rt, &folder);
        fs::write(folder.join("Proposal.docx"), b"first").unwrap();
        wait_until("the file to be versioned", || recent_names(&rt) == vec!["Proposal.docx".to_string()]);
        let mut saw_version = false;
        while let Ok(e) = rx.try_recv() {
            saw_version |= matches!(e, EngineEventDto::VersionCreated { .. });
        }
        assert!(saw_version, "the UI must hear about the new version");
    }

    #[test]
    fn adding_the_same_folder_twice_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("w");
        fs::create_dir(&folder).unwrap();
        let (rt, _rx) = start(dir.path());
        add_and_read(&rt, &folder);
        add_and_read(&rt, &folder);
        assert_eq!(rt.folders().len(), 1);
    }

    #[test]
    fn folders_are_remembered_and_rescanned_on_the_next_launch() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("w");
        fs::create_dir(&folder).unwrap();
        {
            let (rt, _rx) = start(dir.path());
            add_and_read(&rt, &folder);
        }
        fs::write(folder.join("while-closed.txt"), b"saved while Folyvo was not running").unwrap();
        let (rt, _rx) = start(dir.path());
        assert_eq!(rt.folders().len(), 1);
        assert!(rt.folders()[0].problem.is_none());
        wait_until("the startup scan", || recent_names(&rt).contains(&"while-closed.txt".to_string()));
    }

    // Review Focus 2: a folder that is gone at launch must not stop the app or hide the others.
    #[test]
    fn a_folder_that_vanished_is_listed_with_a_problem_and_the_others_still_work() {
        let dir = tempfile::tempdir().unwrap();
        let (gone, ok) = (dir.path().join("gone"), dir.path().join("ok"));
        fs::create_dir(&gone).unwrap();
        fs::create_dir(&ok).unwrap();
        {
            let (rt, _rx) = start(dir.path());
            add_and_read(&rt, &gone);
            add_and_read(&rt, &ok);
        }
        fs::remove_dir_all(&gone).unwrap();
        let (rt, _rx) = start(dir.path());
        let folders = rt.folders();
        assert_eq!(folders.len(), 2);
        let broken: Vec<_> = folders.iter().filter(|f| f.problem.is_some()).collect();
        assert_eq!(broken.len(), 1);
        assert!(broken[0].path.ends_with("gone"));
        fs::write(ok.join("a.txt"), b"x").unwrap();
        wait_until("the healthy folder to keep working", || recent_names(&rt).contains(&"a.txt".to_string()));
    }

    #[test]
    fn a_removed_folder_is_no_longer_watched_but_keeps_its_history() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("w");
        fs::create_dir(&folder).unwrap();
        let (rt, _rx) = start(dir.path());
        add_and_read(&rt, &folder);
        fs::write(folder.join("a.txt"), b"x").unwrap();
        wait_until("first version", || recent_names(&rt).contains(&"a.txt".to_string()));
        rt.remove_folder(&folder).unwrap();
        assert!(rt.folders().is_empty());
        fs::write(folder.join("b.txt"), b"y").unwrap();
        std::thread::sleep(Duration::from_millis(800));
        assert_eq!(recent_names(&rt), vec!["a.txt".to_string()], "history stays, watching stops");
    }

    #[test]
    fn a_file_changed_while_folyvo_was_closed_is_told_once() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("w");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("a.txt"), b"first").unwrap();
        {
            let (rt, _rx) = start(dir.path());
            add_and_read(&rt, &folder);
            assert_eq!(rt.missed_while_closed(), 0, "adding a folder is not a gap");
        }
        fs::write(folder.join("a.txt"), b"second").unwrap();
        fs::write(folder.join("new.txt"), b"nothing was missed of a new file").unwrap();
        {
            let (rt, _rx) = start(dir.path());
            assert_eq!(rt.missed_while_closed(), 1);
            rt.dismiss_missed();
            assert_eq!(rt.missed_while_closed(), 0);
        }
        let (rt, _rx) = start(dir.path());
        assert_eq!(rt.missed_while_closed(), 0, "nothing changed since the last launch");
    }

    #[test]
    fn the_time_left_follows_the_rate_so_far_once_there_is_enough_to_tell() {
        let at = |done, done_bytes| ScanProgress { done, total: 100, done_bytes, total_bytes: 100 };
        assert_eq!(seconds_left(10_000, at(25, 25)), Some(30));
        assert_eq!(seconds_left(10_000, at(100, 100)), Some(0));
        assert_eq!(seconds_left(2_999, at(25, 25)), None, "too early to tell");
        assert_eq!(seconds_left(10_000, at(0, 0)), None, "nothing read yet");
    }

    // A few big files fill the bytes early; thousands of small ones are still ahead.
    #[test]
    fn most_of_the_bytes_done_does_not_promise_the_end_while_most_files_remain() {
        let p = ScanProgress { done: 10, total: 100, done_bytes: 90, total_bytes: 100 };
        assert_eq!(seconds_left(10_000, p), Some(90));
    }

    // A folder can take minutes to read; adding it must not wait, so a failure arrives on its row.
    #[test]
    fn a_folder_that_cannot_be_read_is_added_and_shows_its_problem() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("locked");
        fs::create_dir(&folder).unwrap();
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o000)).unwrap();
        let (rt, _rx) = start(dir.path());
        let added = rt.add_folder(&folder);
        wait_until("the read to end", || rt.folders().iter().all(|f| f.reading.is_none()));
        let problem = rt.folders()[0].problem.clone();
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(added, Ok(()));
        assert!(problem.is_some(), "the row must say what went wrong");
    }

    #[test]
    fn a_folder_being_read_says_so_and_cannot_be_removed_yet() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("w");
        fs::create_dir(&folder).unwrap();
        let (rt, _rx) = start(dir.path());
        add_and_read(&rt, &folder);
        let canonical = fs::canonicalize(&folder).unwrap();
        rt.runners.lock().unwrap().remove(&canonical);
        let progress = ScanProgress { done: 1, total: 4, done_bytes: 25, total_bytes: 100 };
        rt.reading.lock().unwrap().insert(canonical, Reading { started_ms: 0, progress });
        let reading = rt.folders()[0].reading.clone().unwrap();
        assert_eq!((reading.done, reading.total), (1, 4));
        assert_eq!(rt.remove_folder(&folder).unwrap_err().code, "folder_busy");
        assert_eq!(rt.folders().len(), 1);
    }

    #[test]
    fn the_ui_hears_when_a_folder_has_been_read() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("w");
        fs::create_dir(&folder).unwrap();
        let (rt, rx) = start(dir.path());
        add_and_read(&rt, &folder);
        // Stored by `add_folder` and again by the runner, the read started, and the read ended.
        let mut told = 0;
        wait_until("the last notice", || {
            while let Ok(e) = rx.try_recv() {
                told += usize::from(matches!(e, EngineEventDto::RootsChanged));
            }
            told >= 4
        });
    }

    #[test]
    fn a_folder_inside_the_vault_is_refused_with_a_plain_message() {
        let dir = tempfile::tempdir().unwrap();
        let (rt, _rx) = start(dir.path());
        let err = rt.add_folder(&dir.path().join("vault")).unwrap_err();
        assert_eq!(err.code, "folder_not_allowed");
    }
}
