use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
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
use crate::dto::{EngineEventDto, FolderDto, ReadingDto, ReadingState};
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

/// How long a paused or stopped read keeps what it read, in case the folder comes back. After that
/// it is taken back: half a folder must not sit in the vault for good, looking kept.
const PAUSE_LIMIT_MS: i64 = 24 * 60 * 60 * 1_000;

/// How long the popover brings up a paused read. It must not nag for the whole day.
const PAUSE_SHOWN_MS: i64 = 30 * 60 * 1_000;

/// How often a running Folyvo looks at its paused reads.
const PAUSE_CHECK_EVERY: Duration = Duration::from_secs(60);

/// What the person asked of a read in progress.
#[derive(Default)]
struct Stop {
    /// End after the file it is on.
    now: AtomicBool,
    /// And take back what it added, with the folder. Without this, the read is paused.
    remove: AtomicBool,
}

impl Stop {
    fn already_stopped() -> Stop {
        Stop { now: AtomicBool::new(true), remove: AtomicBool::new(false) }
    }
}

/// How `start_runner` left a folder.
#[derive(Debug, PartialEq, Eq)]
enum Started {
    /// Read to the end and being watched; holds how many kept files the scan found changed.
    Watching(usize),
    /// Watched, with the rest of the read still to come.
    Paused,
    /// Gone, with what the read added.
    Removed,
}

struct Reading {
    started_ms: i64,
    progress: ScanProgress,
    state: ReadingState,
    paused_at: Option<i64>,
    stop: Arc<Stop>,
}

impl Reading {
    fn dto(&self, now_ms: i64) -> ReadingDto {
        let p = self.progress;
        ReadingDto {
            done: p.done as u32,
            total: p.total as u32,
            seconds_left: (self.state == ReadingState::Reading)
                .then(|| seconds_left(now_ms - self.started_ms, p))
                .flatten(),
            state: self.state,
            quiet: self.state == ReadingState::Paused
                && self.paused_at.is_some_and(|at| now_ms - at >= PAUSE_SHOWN_MS),
        }
    }
}

pub struct AppRuntime {
    engine: Arc<Engine>,
    clock: Arc<dyn Clock>,
    quiet: (i64, i64),
    runners: Mutex<HashMap<PathBuf, Runner>>,
    problems: Mutex<HashMap<PathBuf, UserError>>,
    /// Folders whose first read has not ended, paused ones too. Reading one can take minutes.
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
        if let Err(problem) = runtime.forget_pauses_that_ran_out() {
            eprintln!("folyvo: could not take back a paused read: {}", problem.title);
        }
        let unfinished = runtime.engine.unfinished_reads()?;
        let paused = runtime.engine.paused_reads()?;
        for root in runtime.engine.roots()? {
            let paused_at = paused.iter().find(|(path, _)| *path == root).map(|(_, at)| *at);
            let problem = if !unfinished.contains(&root) {
                match runtime.start_runner(&root, &|_| {}, &Stop::default()) {
                    Ok(Started::Watching(changed)) => {
                        runtime.missed.fetch_add(changed, Ordering::SeqCst);
                        None
                    }
                    Ok(_) => None,
                    Err(problem) => Some(problem),
                }
            } else if let Some(at) = paused_at {
                runtime.stay_paused(&root, at).err()
            } else {
                // Folyvo closed during this read. It goes on behind the menu bar.
                runtime.read_in_background(root.clone());
                None
            };
            if let Some(problem) = problem {
                runtime.problems.lock().unwrap().insert(root, problem);
            }
        }
        let weak = Arc::downgrade(&runtime);
        thread::spawn(move || look_after_paused_reads(weak));
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
        let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
        let state = self.reading.lock().unwrap().get(&canonical).map(|r| r.state);
        match state {
            // Added again while paused: the person wants the rest.
            Some(ReadingState::Paused) => return self.resume_folder(folder),
            Some(_) => return Ok(()),
            None => {}
        }
        if self.runners.lock().unwrap().contains_key(&canonical) {
            return Ok(());
        }
        self.engine.begin_read(folder)?;
        self.problems.lock().unwrap().remove(folder);
        self.problems.lock().unwrap().remove(&canonical);
        self.read_in_background(canonical);
        Ok(())
    }

    fn read_in_background(self: &Arc<Self>, canonical: PathBuf) {
        let stop = Arc::new(Stop::default());
        {
            let mut reading = self.reading.lock().unwrap();
            if reading.contains_key(&canonical) {
                return;
            }
            let started_ms = self.clock.now_unix_ms();
            reading.insert(
                canonical.clone(),
                Reading {
                    started_ms,
                    progress: ScanProgress::default(),
                    state: ReadingState::Reading,
                    paused_at: None,
                    stop: stop.clone(),
                },
            );
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
            let started = rt.start_runner(&canonical, &on_progress, &stop);
            if matches!(started, Ok(Started::Paused)) {
                if let Some(reading) = rt.reading.lock().unwrap().get_mut(&canonical) {
                    reading.state = ReadingState::Paused;
                }
                // Asked to take the folder away just as the pause took hold.
                if stop.remove.load(Ordering::SeqCst) && rt.remove_folder(&canonical).is_err() {
                    eprintln!("folyvo: could not remove {}", canonical.display());
                }
            } else {
                if let Err(problem) = started {
                    rt.problems.lock().unwrap().insert(canonical.clone(), problem);
                }
                rt.reading.lock().unwrap().remove(&canonical);
            }
            rt.notifier.notify(EngineEventDto::RootsChanged);
        });
    }

    /// Watches a folder whose read was paused before Folyvo closed, without reading any more of it.
    fn stay_paused(&self, root: &Path, paused_at: i64) -> Result<(), UserError> {
        let stop = Arc::new(Stop::already_stopped());
        let reading = Reading {
            started_ms: self.clock.now_unix_ms(),
            progress: ScanProgress::default(),
            state: ReadingState::Paused,
            paused_at: Some(paused_at),
            stop: stop.clone(),
        };
        self.reading.lock().unwrap().insert(root.to_path_buf(), reading);
        self.start_runner(root, &|_| {}, &stop).map(|_| ()).inspect_err(|_| {
            self.reading.lock().unwrap().remove(root);
        })
    }

    /// Ends a read after the file it is on. What was read stays, and the folder goes on being
    /// watched, so no save is lost. The rest waits for `resume_folder`; past the time limit, what
    /// was read is taken back.
    pub fn pause_folder(&self, folder: &Path) -> Result<(), UserError> {
        let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
        let mut reading = self.reading.lock().unwrap();
        let Some(read) = reading.get_mut(&canonical) else { return Ok(()) };
        if read.state != ReadingState::Reading {
            return Ok(());
        }
        self.engine.pause_read(&canonical)?;
        read.state = ReadingState::Pausing;
        read.paused_at = Some(self.clock.now_unix_ms());
        read.stop.now.store(true, Ordering::SeqCst);
        drop(reading);
        self.notifier.notify(EngineEventDto::RootsChanged);
        Ok(())
    }

    /// Takes a paused folder out of the list and stops watching it. What was read waits until the
    /// time limit: adding the folder again goes on from there.
    pub fn stop_reading(&self, folder: &Path) -> Result<(), UserError> {
        let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
        {
            let mut reading = self.reading.lock().unwrap();
            if !reading.get(&canonical).is_some_and(|r| r.state == ReadingState::Paused) {
                return Ok(());
            }
            reading.remove(&canonical);
        }
        drop(self.runners.lock().unwrap().remove(&canonical)); // stops the watcher thread
        self.problems.lock().unwrap().remove(&canonical);
        self.engine.stop_read(&canonical)?;
        Ok(())
    }

    /// Goes on with a paused read. Files already read are not stored again.
    pub fn resume_folder(self: &Arc<Self>, folder: &Path) -> Result<(), UserError> {
        let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
        {
            let mut reading = self.reading.lock().unwrap();
            if !reading.get(&canonical).is_some_and(|r| r.state == ReadingState::Paused) {
                return Ok(());
            }
            reading.remove(&canonical);
        }
        self.engine.resume_read(&canonical)?;
        // The read watches again from its own start, so nothing saved in the gap is missed.
        drop(self.runners.lock().unwrap().remove(&canonical));
        self.read_in_background(canonical);
        Ok(())
    }

    fn forget_pauses_that_ran_out(&self) -> Result<(), UserError> {
        let now = self.clock.now_unix_ms();
        for (root, at) in self.engine.paused_reads()? {
            if now - at >= PAUSE_LIMIT_MS {
                self.remove_folder(&root)?;
            }
        }
        Ok(())
    }

    /// A folder still being read is asked to stop; it goes when the read ends. A read that did not
    /// finish gives back what it added. Anything else keeps its history.
    pub fn remove_folder(&self, folder: &Path) -> Result<(), UserError> {
        let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
        {
            let mut reading = self.reading.lock().unwrap();
            match reading.get_mut(&canonical) {
                Some(read) if read.state != ReadingState::Paused => {
                    read.state = ReadingState::Stopping;
                    read.stop.remove.store(true, Ordering::SeqCst);
                    read.stop.now.store(true, Ordering::SeqCst);
                    drop(reading);
                    self.notifier.notify(EngineEventDto::RootsChanged);
                    return Ok(());
                }
                Some(_) => {
                    reading.remove(&canonical);
                }
                None => {}
            }
        }
        let runner = self.runners.lock().unwrap().remove(&canonical);
        drop(runner); // stops the watcher thread
        self.problems.lock().unwrap().remove(folder);
        self.problems.lock().unwrap().remove(&canonical);
        self.engine.forget_read(folder)?;
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

    fn start_runner(
        &self,
        folder: &Path,
        on_progress: &dyn Fn(ScanProgress),
        stop: &Stop,
    ) -> Result<Started, UserError> {
        let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
        if self.runners.lock().unwrap().contains_key(&canonical) {
            return Ok(Started::Watching(0));
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
            &stop.now,
        )?;
        if report.stopped && stop.remove.load(Ordering::SeqCst) {
            drop(runner); // stops the watcher thread
            self.engine.forget_read(folder)?;
            self.engine.remove_root(folder)?;
            self.notifier.notify(EngineEventDto::Resync);
            return Ok(Started::Removed);
        }
        if report.stopped {
            self.runners.lock().unwrap().insert(runner.folder().to_path_buf(), runner);
            return Ok(Started::Paused);
        }
        // Only the folder itself: one unreadable folder inside it must not stop the rest being kept.
        if report.failed.iter().any(|(path, _)| path == runner.folder()) {
            let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
            return Err(CoreError::io(runner.folder(), denied).into());
        }
        self.engine.finish_read(runner.folder())?;
        self.runners.lock().unwrap().insert(runner.folder().to_path_buf(), runner);
        Ok(Started::Watching(report.changed))
    }
}

/// Runs for as long as Folyvo does, so a pause cannot outlast its limit while the app stays open
/// and the popover hears when one has been showing long enough.
fn look_after_paused_reads(runtime: Weak<AppRuntime>) {
    loop {
        thread::sleep(PAUSE_CHECK_EVERY);
        let Some(runtime) = runtime.upgrade() else { return };
        if let Err(problem) = runtime.forget_pauses_that_ran_out() {
            eprintln!("folyvo: could not take back a paused read: {}", problem.title);
        }
        let any_paused = runtime.reading.lock().unwrap().values().any(|r| r.state == ReadingState::Paused);
        if any_paused {
            runtime.notifier.notify(EngineEventDto::RootsChanged);
        }
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
    fn a_read_cut_short_goes_on_behind_the_menu_bar_when_folyvo_opens_again() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("w");
        fs::create_dir(&folder).unwrap();
        for i in 0..500 {
            fs::write(folder.join(format!("{i}.txt")), i.to_string()).unwrap();
        }
        {
            let (rt, _rx) = start(dir.path());
            rt.engine.begin_read(&folder).unwrap(); // and Folyvo closes before the read ends
        }
        let (rt, _rx) = start(dir.path());
        assert!(rt.folders()[0].reading.is_some(), "opening must not wait for the read");
        wait_until("the read to end", || rt.folders()[0].reading.is_none());
        assert_eq!(rt.api().recent(1_000).unwrap().len(), 500);
        assert!(rt.engine.unfinished_reads().unwrap().is_empty());
    }

    #[test]
    fn a_finished_read_is_not_read_again_as_unfinished() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("w");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("a.txt"), b"one").unwrap();
        {
            let (rt, _rx) = start(dir.path());
            add_and_read(&rt, &folder);
        }
        let (rt, _rx) = start(dir.path());
        assert!(rt.folders()[0].reading.is_none());
        assert_eq!(recent_names(&rt), vec!["a.txt".to_string()]);
    }

    #[test]
    fn stopping_takes_back_what_was_read_before_folyvo_closed() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("w");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("a.txt"), b"one").unwrap();
        let (rt, _rx) = start(dir.path());
        rt.engine.begin_read(&folder).unwrap();
        folyvo_core::rescan::rescan_root(&rt.engine, &fs::canonicalize(&folder).unwrap()).unwrap(); // the earlier session
        fs::write(folder.join("b.txt"), b"two").unwrap();
        let stop = Stop { remove: AtomicBool::new(true), ..Stop::default() };
        let on_progress = |_: ScanProgress| stop.now.store(true, Ordering::SeqCst);
        assert_eq!(rt.start_runner(&folder, &on_progress, &stop), Ok(Started::Removed));
        assert!(rt.folders().is_empty());
        assert_eq!(rt.engine.vault_stats().unwrap().items, 0);
        assert_eq!(fs::read(folder.join("a.txt")).unwrap(), b"one");
    }

    #[test]
    fn removing_a_folder_whose_read_could_not_end_takes_back_what_it_added() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("w");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("a.txt"), b"one").unwrap();
        let (rt, _rx) = start(dir.path());
        rt.engine.begin_read(&folder).unwrap();
        folyvo_core::rescan::rescan_root(&rt.engine, &fs::canonicalize(&folder).unwrap()).unwrap();
        rt.remove_folder(&folder).unwrap();
        assert_eq!(rt.engine.vault_stats().unwrap().items, 0);
        assert!(rt.folders().is_empty());
    }

    fn start_at(dir: &std::path::Path, clock: Arc<folyvo_core::ports::FakeClock>) -> Arc<AppRuntime> {
        let (tx, _rx) = mpsc::channel();
        AppRuntime::start(
            &dir.join("vault"),
            clock,
            Arc::new(RandomIdGen),
            (100, 100),
            Arc::new(Collect(Mutex::new(tx))),
        )
        .unwrap()
    }

    fn state(rt: &AppRuntime) -> Option<ReadingState> {
        rt.folders()[0].reading.as_ref().map(|r| r.state)
    }

    /// A folder of two files, its read paused after the first, as Folyvo closes.
    fn paused_folder(dir: &std::path::Path, clock: &Arc<folyvo_core::ports::FakeClock>) -> PathBuf {
        let folder = dir.join("w");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("a.txt"), b"one").unwrap();
        fs::write(folder.join("b.txt"), b"two").unwrap();
        let rt = start_at(dir, clock.clone());
        rt.engine.begin_read(&folder).unwrap();
        let stop = Stop::default();
        let on_progress = |p: ScanProgress| {
            if p.done >= 1 {
                stop.now.store(true, Ordering::SeqCst);
            }
        };
        assert_eq!(rt.start_runner(&folder, &on_progress, &stop), Ok(Started::Paused));
        rt.engine.pause_read(&fs::canonicalize(&folder).unwrap()).unwrap();
        assert_eq!(rt.engine.vault_stats().unwrap().items, 1);
        folder
    }

    #[test]
    fn a_paused_read_keeps_what_it_read_and_goes_on_watching() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("w");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("a.txt"), b"one").unwrap();
        fs::write(folder.join("b.txt"), b"two").unwrap();
        let (rt, _rx) = start(dir.path());
        rt.engine.begin_read(&folder).unwrap();
        let stop = Stop::default();
        let on_progress = |p: ScanProgress| {
            if p.done >= 1 {
                stop.now.store(true, Ordering::SeqCst);
            }
        };
        assert_eq!(rt.start_runner(&folder, &on_progress, &stop), Ok(Started::Paused));
        assert_eq!(rt.engine.vault_stats().unwrap().items, 1);
        assert_eq!(rt.engine.unfinished_reads().unwrap().len(), 1);

        fs::write(folder.join("a.txt"), b"saved while paused").unwrap();
        wait_until("the save to be kept", || rt.engine.vault_stats().unwrap().versions == 2);
    }

    #[test]
    fn a_pause_lasts_across_opening_folyvo_again() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(folyvo_core::ports::FakeClock::new(1_000));
        paused_folder(dir.path(), &clock);
        clock.advance(PAUSE_LIMIT_MS - 1);
        let rt = start_at(dir.path(), clock);
        assert_eq!(state(&rt), Some(ReadingState::Paused));
        assert_eq!(rt.engine.vault_stats().unwrap().items, 1);
        assert_eq!(rt.runners.lock().unwrap().len(), 1, "still watched");
    }

    #[test]
    fn resuming_finishes_the_read_without_storing_anything_twice() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(folyvo_core::ports::FakeClock::new(1_000));
        let folder = paused_folder(dir.path(), &clock);
        let rt = start_at(dir.path(), clock);
        rt.resume_folder(&folder).unwrap();
        wait_until("the read to end", || state(&rt).is_none());
        let stats = rt.engine.vault_stats().unwrap();
        assert_eq!((stats.items, stats.versions), (2, 2));
        assert!(rt.engine.unfinished_reads().unwrap().is_empty());
        assert_eq!(rt.api().recent(10).unwrap().len(), 2);
    }

    #[test]
    fn a_pause_that_runs_out_while_folyvo_is_closed_is_taken_back_on_opening() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(folyvo_core::ports::FakeClock::new(1_000));
        let folder = paused_folder(dir.path(), &clock);
        clock.advance(PAUSE_LIMIT_MS);
        let rt = start_at(dir.path(), clock);
        assert!(rt.folders().is_empty());
        assert_eq!(rt.engine.vault_stats().unwrap().items, 0);
        assert!(rt.engine.paused_reads().unwrap().is_empty());
        assert_eq!(fs::read(folder.join("a.txt")).unwrap(), b"one", "the person's own file stays");
    }

    #[test]
    fn a_pause_that_runs_out_while_folyvo_is_open_is_taken_back() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(folyvo_core::ports::FakeClock::new(1_000));
        paused_folder(dir.path(), &clock);
        let rt = start_at(dir.path(), clock.clone());
        rt.forget_pauses_that_ran_out().unwrap();
        assert_eq!(state(&rt), Some(ReadingState::Paused), "the limit has not come");
        clock.advance(PAUSE_LIMIT_MS);
        rt.forget_pauses_that_ran_out().unwrap();
        assert!(rt.folders().is_empty());
        assert!(rt.runners.lock().unwrap().is_empty());
        assert_eq!(rt.engine.vault_stats().unwrap().items, 0);
    }

    #[test]
    fn a_file_saved_during_a_pause_outlives_the_pause_as_history() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(folyvo_core::ports::FakeClock::new(1_000));
        let folder = paused_folder(dir.path(), &clock);
        let rt = start_at(dir.path(), clock.clone());
        let first = rt.engine.vault_stats().unwrap().items;
        assert_eq!(first, 1);
        fs::write(folder.join("a.txt"), b"saved while paused").unwrap();
        folyvo_core::rescan::rescan_root(&rt.engine, &fs::canonicalize(&folder).unwrap()).unwrap();
        clock.advance(PAUSE_LIMIT_MS);
        rt.forget_pauses_that_ran_out().unwrap();
        assert!(rt.folders().is_empty());
        assert_eq!(rt.engine.vault_stats().unwrap().items, 1, "a.txt has two saves and stays");
        assert_eq!(rt.api().recent(10).unwrap()[0].name, "a.txt");
    }

    #[test]
    fn the_popover_stops_bringing_up_a_pause_after_half_an_hour() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(folyvo_core::ports::FakeClock::new(1_000));
        paused_folder(dir.path(), &clock);
        clock.advance(PAUSE_SHOWN_MS - 1);
        let rt = start_at(dir.path(), clock.clone());
        assert!(!rt.folders()[0].reading.clone().unwrap().quiet);
        clock.advance(1);
        let reading = rt.folders()[0].reading.clone().unwrap();
        assert!(reading.quiet);
        assert_eq!(reading.state, ReadingState::Paused, "only the popover goes quiet");
    }

    #[test]
    fn stopping_a_paused_read_closes_it_at_once_and_stops_watching() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(folyvo_core::ports::FakeClock::new(1_000));
        let folder = paused_folder(dir.path(), &clock);
        let rt = start_at(dir.path(), clock);
        rt.stop_reading(&folder).unwrap();
        assert!(rt.folders().is_empty());
        assert!(rt.runners.lock().unwrap().is_empty());
        assert_eq!(rt.engine.vault_stats().unwrap().items, 1, "what was read waits");
        assert!(rt.api().recent(10).unwrap().is_empty());
    }

    #[test]
    fn adding_a_stopped_folder_again_goes_on_with_its_read() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(folyvo_core::ports::FakeClock::new(1_000));
        let folder = paused_folder(dir.path(), &clock);
        let rt = start_at(dir.path(), clock.clone());
        rt.stop_reading(&folder).unwrap();
        clock.advance(PAUSE_LIMIT_MS - 1);
        rt.add_folder(&folder).unwrap();
        assert_eq!(rt.folders().len(), 1);
        wait_until("the read to end", || state(&rt).is_none());
        let stats = rt.engine.vault_stats().unwrap();
        assert_eq!((stats.items, stats.versions), (2, 2), "nothing is stored twice");
        assert!(rt.engine.unfinished_reads().unwrap().is_empty());
    }

    #[test]
    fn a_stopped_read_that_runs_out_is_taken_back_and_starts_afresh() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(folyvo_core::ports::FakeClock::new(1_000));
        let folder = paused_folder(dir.path(), &clock);
        let rt = start_at(dir.path(), clock.clone());
        rt.stop_reading(&folder).unwrap();
        clock.advance(PAUSE_LIMIT_MS);
        rt.forget_pauses_that_ran_out().unwrap();
        assert_eq!(rt.engine.vault_stats().unwrap().items, 0);
        assert!(rt.engine.paused_reads().unwrap().is_empty());
    }

    #[test]
    fn a_stopped_folder_stays_out_of_the_list_on_opening_folyvo_again() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(folyvo_core::ports::FakeClock::new(1_000));
        let folder = paused_folder(dir.path(), &clock);
        {
            let rt = start_at(dir.path(), clock.clone());
            rt.stop_reading(&folder).unwrap();
        }
        let rt = start_at(dir.path(), clock);
        assert!(rt.folders().is_empty());
        assert!(rt.runners.lock().unwrap().is_empty());
    }

    #[test]
    fn adding_a_paused_folder_again_resumes_it() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(folyvo_core::ports::FakeClock::new(1_000));
        let folder = paused_folder(dir.path(), &clock);
        let rt = start_at(dir.path(), clock);
        rt.add_folder(&folder).unwrap();
        wait_until("the read to end", || state(&rt).is_none());
        assert_eq!(rt.engine.vault_stats().unwrap().items, 2);
    }

    #[test]
    fn removing_a_paused_folder_takes_back_what_it_read() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(folyvo_core::ports::FakeClock::new(1_000));
        let folder = paused_folder(dir.path(), &clock);
        let rt = start_at(dir.path(), clock);
        rt.remove_folder(&folder).unwrap();
        assert!(rt.folders().is_empty());
        assert!(rt.runners.lock().unwrap().is_empty());
        assert_eq!(rt.engine.vault_stats().unwrap().items, 0);
        assert_eq!(fs::read(folder.join("a.txt")).unwrap(), b"one");
    }

    #[test]
    fn pausing_a_folder_that_is_not_being_read_does_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("w");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("a.txt"), b"one").unwrap();
        let (rt, _rx) = start(dir.path());
        add_and_read(&rt, &folder);
        rt.pause_folder(&folder).unwrap();
        rt.resume_folder(&folder).unwrap();
        assert!(rt.engine.paused_reads().unwrap().is_empty());
        assert_eq!(rt.runners.lock().unwrap().len(), 1);
    }

    #[test]
    fn stopping_a_read_leaves_the_folder_as_if_it_had_never_been_added() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("w");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("a.txt"), b"one").unwrap();
        let (rt, _rx) = start(dir.path());
        let stop = Stop { remove: AtomicBool::new(true), ..Stop::default() };
        let on_progress = |_: ScanProgress| stop.now.store(true, Ordering::SeqCst);
        assert_eq!(rt.start_runner(&folder, &on_progress, &stop), Ok(Started::Removed));
        assert!(rt.folders().is_empty());
        assert!(rt.runners.lock().unwrap().is_empty());
        assert!(recent_names(&rt).is_empty());
        assert_eq!(fs::read(folder.join("a.txt")).unwrap(), b"one");
    }

    #[test]
    fn a_folder_being_read_says_so_and_stopping_it_asks_the_read_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("w");
        fs::create_dir(&folder).unwrap();
        let (rt, _rx) = start(dir.path());
        add_and_read(&rt, &folder);
        let canonical = fs::canonicalize(&folder).unwrap();
        rt.runners.lock().unwrap().remove(&canonical);
        let progress = ScanProgress { done: 1, total: 4, done_bytes: 25, total_bytes: 100 };
        let stop = Arc::new(Stop::default());
        let read = Reading {
            started_ms: 0,
            progress,
            state: ReadingState::Reading,
            paused_at: None,
            stop: stop.clone(),
        };
        rt.reading.lock().unwrap().insert(canonical, read);
        let reading = rt.folders()[0].reading.clone().unwrap();
        assert_eq!((reading.done, reading.total, reading.state), (1, 4, ReadingState::Reading));
        assert_eq!(rt.remove_folder(&folder), Ok(()));
        assert!(stop.now.load(Ordering::SeqCst) && stop.remove.load(Ordering::SeqCst));
        assert_eq!(rt.folders().len(), 1, "the row stays until the read has ended");
        assert_eq!(state(&rt), Some(ReadingState::Stopping));
    }

    #[test]
    fn pausing_asks_the_read_to_end_and_keeps_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("w");
        fs::create_dir(&folder).unwrap();
        let (rt, _rx) = start(dir.path());
        rt.engine.begin_read(&folder).unwrap();
        let canonical = fs::canonicalize(&folder).unwrap();
        let stop = Arc::new(Stop::default());
        let read = Reading {
            started_ms: 0,
            progress: ScanProgress::default(),
            state: ReadingState::Reading,
            paused_at: None,
            stop: stop.clone(),
        };
        rt.reading.lock().unwrap().insert(canonical.clone(), read);
        assert_eq!(rt.pause_folder(&folder), Ok(()));
        assert!(stop.now.load(Ordering::SeqCst));
        assert!(!stop.remove.load(Ordering::SeqCst));
        assert_eq!(state(&rt), Some(ReadingState::Pausing));
        assert_eq!(rt.engine.paused_reads().unwrap().len(), 1);
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
