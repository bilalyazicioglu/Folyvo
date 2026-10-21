use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use folyvo_core::engine::{Engine, RecvError};
use folyvo_core::ports::{Clock, IdGen};
use folyvo_core::runner::{OnApplied, Runner};
use folyvo_core::watch::Applied;

use crate::api::AppApi;
use crate::dto::{EngineEventDto, FolderDto};
use crate::errors::UserError;

/// Receives every event the UI should hear about. The Tauri layer forwards them to the webviews.
pub trait Notifier: Send + Sync {
    fn notify(&self, event: EngineEventDto);
}

pub struct AppRuntime {
    engine: Arc<Engine>,
    clock: Arc<dyn Clock>,
    quiet: (i64, i64),
    runners: Mutex<HashMap<PathBuf, Runner>>,
    problems: Mutex<HashMap<PathBuf, UserError>>,
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
            engine,
            clock,
            quiet,
            runners: Mutex::new(HashMap::new()),
            problems: Mutex::new(HashMap::new()),
        });
        for root in runtime.engine.roots()? {
            if let Err(problem) = runtime.start_runner(&root) {
                runtime.problems.lock().unwrap().insert(root, problem);
            }
        }
        Ok(runtime)
    }

    pub fn api(&self) -> AppApi {
        AppApi::new(self.engine.clone())
    }

    pub fn add_folder(&self, folder: &Path) -> Result<(), UserError> {
        self.start_runner(folder)?;
        self.problems.lock().unwrap().remove(folder);
        Ok(())
    }

    pub fn remove_folder(&self, folder: &Path) -> Result<(), UserError> {
        let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
        let runner = self.runners.lock().unwrap().remove(&canonical);
        drop(runner); // stops the watcher thread
        self.problems.lock().unwrap().remove(folder);
        self.problems.lock().unwrap().remove(&canonical);
        self.engine.remove_root(folder)?;
        Ok(())
    }

    pub fn folders(&self) -> Vec<FolderDto> {
        let problems = self.problems.lock().unwrap();
        self.engine
            .roots()
            .unwrap_or_default()
            .into_iter()
            .map(|root| FolderDto {
                problem: problems.get(&root).cloned(),
                path: root.to_string_lossy().into_owned(),
            })
            .collect()
    }

    fn start_runner(&self, folder: &Path) -> Result<(), UserError> {
        let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
        if self.runners.lock().unwrap().contains_key(&canonical) {
            return Ok(());
        }
        let on_applied: OnApplied = Arc::new(|applied| {
            if let Applied::Failed { path, reason } = applied {
                eprintln!("folyvo: could not save {}: {reason}", path.display());
            }
        });
        let (runner, _report) =
            Runner::start(self.engine.clone(), self.clock.clone(), folder, self.quiet, on_applied)?;
        self.runners.lock().unwrap().insert(runner.folder().to_path_buf(), runner);
        Ok(())
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

    fn recent_names(rt: &AppRuntime) -> Vec<String> {
        rt.api().recent(50).unwrap().into_iter().map(|i| i.name).collect()
    }

    #[test]
    fn a_file_saved_in_an_added_folder_becomes_a_version_and_an_event() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Client work");
        fs::create_dir(&folder).unwrap();
        let (rt, rx) = start(dir.path());
        rt.add_folder(&folder).unwrap();
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
        rt.add_folder(&folder).unwrap();
        rt.add_folder(&folder).unwrap();
        assert_eq!(rt.folders().len(), 1);
    }

    #[test]
    fn folders_are_remembered_and_rescanned_on_the_next_launch() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("w");
        fs::create_dir(&folder).unwrap();
        {
            let (rt, _rx) = start(dir.path());
            rt.add_folder(&folder).unwrap();
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
            rt.add_folder(&gone).unwrap();
            rt.add_folder(&ok).unwrap();
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
        rt.add_folder(&folder).unwrap();
        fs::write(folder.join("a.txt"), b"x").unwrap();
        wait_until("first version", || recent_names(&rt).contains(&"a.txt".to_string()));
        rt.remove_folder(&folder).unwrap();
        assert!(rt.folders().is_empty());
        fs::write(folder.join("b.txt"), b"y").unwrap();
        std::thread::sleep(Duration::from_millis(800));
        assert_eq!(recent_names(&rt), vec!["a.txt".to_string()], "history stays, watching stops");
    }

    #[test]
    fn a_folder_inside_the_vault_is_refused_with_a_plain_message() {
        let dir = tempfile::tempdir().unwrap();
        let (rt, _rx) = start(dir.path());
        let err = rt.add_folder(&dir.path().join("vault")).unwrap_err();
        assert_eq!(err.code, "folder_not_allowed");
    }
}
