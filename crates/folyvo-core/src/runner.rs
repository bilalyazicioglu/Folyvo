use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use notify::{EventKind, RecursiveMode, Watcher};

use crate::debounce::{FsEvent, KNOWN_QUIET_MS, NEW_QUIET_MS};
use crate::engine::Engine;
use crate::error::{CoreError, Result};
use crate::ports::Clock;
use crate::rescan::{rescan_root, RescanReport};
use crate::watch::{Applied, WatchLoop};

pub const DEFAULT_QUIET: (i64, i64) = (KNOWN_QUIET_MS, NEW_QUIET_MS);

pub type OnApplied = Arc<dyn Fn(&Applied) + Send + Sync>;

/// Watches one folder and turns saves into versions until stopped.
pub struct Runner {
    folder: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Runner {
    pub fn start(
        engine: Arc<Engine>,
        clock: Arc<dyn Clock>,
        folder: &Path,
        quiet: (i64, i64),
        on_applied: OnApplied,
    ) -> Result<(Runner, RescanReport)> {
        let folder = std::fs::canonicalize(folder).map_err(|e| CoreError::io(folder, e))?;
        engine.add_root(&folder)?;

        let (tx, rx) = mpsc::channel::<(PathBuf, FsEvent)>();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            for path in event.paths {
                // Renames arrive as separate events per path; the file system decides what each means.
                let kind = if path.exists() { FsEvent::Changed } else { FsEvent::Removed };
                let _ = tx.send((path, kind));
            }
        })
        .map_err(|e| CoreError::Watch(e.to_string()))?;
        watcher.watch(&folder, RecursiveMode::Recursive).map_err(|e| CoreError::Watch(e.to_string()))?;

        // The watcher is live before the scan starts: a file saved while the scan runs is queued
        // as an event instead of being lost in the gap between the two.
        let report = rescan_root(&engine, &folder)?;

        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let mut watch = WatchLoop::new(engine, clock, quiet.0, quiet.1);
        let thread = thread::spawn(move || {
            let _keep_alive = watcher;
            while !flag.load(Ordering::SeqCst) {
                let wait = watch.next_deadline_in_ms().map(|ms| ms.clamp(10, 200)).unwrap_or(200);
                match rx.recv_timeout(Duration::from_millis(wait as u64)) {
                    Ok((path, kind)) => watch.record(&path, kind),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
                while let Ok((path, kind)) = rx.try_recv() {
                    watch.record(&path, kind);
                }
                if flag.load(Ordering::SeqCst) {
                    return;
                }
                for applied in watch.tick() {
                    on_applied(&applied);
                }
            }
        });
        Ok((Runner { folder, stop, thread: Some(thread) }, report))
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.shutdown();
    }
}
