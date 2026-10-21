use std::fs;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use folyvo_core::engine::Engine;
use folyvo_core::ports::{RandomIdGen, SystemClock};
use folyvo_core::runner::{OnApplied, Runner};
use folyvo_core::watch::Applied;

fn engine(dir: &std::path::Path) -> Arc<Engine> {
    Arc::new(Engine::open(&dir.join("vault"), Arc::new(SystemClock), Arc::new(RandomIdGen)).unwrap())
}

fn collector() -> (OnApplied, mpsc::Receiver<String>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let cb: OnApplied = Arc::new(move |a: &Applied| {
        let line = match a {
            Applied::Saved { path, version } => {
                format!("saved {} v{}", path.file_name().unwrap().to_string_lossy(), version.seq)
            }
            Applied::Missing { path } => format!("missing {}", path.file_name().unwrap().to_string_lossy()),
            _ => return,
        };
        let _ = tx.lock().unwrap().send(line);
    });
    (cb, rx)
}

fn wait_for(rx: &mpsc::Receiver<String>, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(200)) {
            if line == needle {
                return;
            }
        }
    }
    panic!("never saw {needle:?}");
}

#[test]
fn the_startup_scan_reports_existing_files_before_start_returns() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("work");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("a.txt"), b"hello").unwrap();
    let (cb, _rx) = collector();
    let (runner, report) =
        Runner::start(engine(dir.path()), Arc::new(SystemClock), &folder, (100, 100), cb).unwrap();
    assert_eq!(report.created, 1);
    runner.stop();
}

#[test]
fn files_saved_after_start_become_versions_and_removal_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("work");
    fs::create_dir(&folder).unwrap();
    let (cb, rx) = collector();
    let (runner, _) =
        Runner::start(engine(dir.path()), Arc::new(SystemClock), &folder, (100, 100), cb).unwrap();

    let f = folder.join("Proposal.docx");
    fs::write(&f, b"first").unwrap();
    wait_for(&rx, "saved Proposal.docx v1");
    fs::write(&f, b"second, longer").unwrap();
    wait_for(&rx, "saved Proposal.docx v2");
    fs::remove_file(&f).unwrap();
    wait_for(&rx, "missing Proposal.docx");
    runner.stop();
}

#[test]
fn nothing_is_reported_after_stop_returns() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("work");
    fs::create_dir(&folder).unwrap();
    let (cb, rx) = collector();
    let (runner, _) =
        Runner::start(engine(dir.path()), Arc::new(SystemClock), &folder, (100, 100), cb).unwrap();
    let started = Instant::now();
    runner.stop();
    assert!(started.elapsed() < Duration::from_secs(2), "stop must not hang");
    fs::write(folder.join("late.txt"), b"x").unwrap();
    std::thread::sleep(Duration::from_millis(600));
    assert!(rx.try_recv().is_err(), "a stopped runner must stay silent");
}

#[test]
fn a_folder_that_does_not_exist_is_an_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let (cb, _rx) = collector();
    let err =
        Runner::start(engine(dir.path()), Arc::new(SystemClock), &dir.path().join("nope"), (100, 100), cb)
            .err()
            .expect("must fail");
    assert!(!err.to_string().is_empty());
}

#[test]
fn a_folder_inside_the_vault_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let (cb, _rx) = collector();
    let err = Runner::start(e, Arc::new(SystemClock), &dir.path().join("vault"), (100, 100), cb)
        .err()
        .expect("must fail");
    assert!(matches!(err, folyvo_core::CoreError::RootOverlapsVault(_)), "{err}");
}
