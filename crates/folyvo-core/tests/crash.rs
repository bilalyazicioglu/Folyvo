use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};

use std::sync::Arc;

use folyvo_core::cas::Cas;
use folyvo_core::engine::Engine;
use folyvo_core::model::ContentHash;
use folyvo_core::ports::{RandomIdGen, SystemClock};

#[test]
fn a_power_cut_mid_write_leaves_no_object_and_the_sweep_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let cas = Cas::open(&dir.path().join("vault")).unwrap();
    let src = dir.path().join("a.docx");
    fs::write(&src, b"important bytes").unwrap();
    let hash = ContentHash::of_bytes(b"important bytes");

    let crashed = catch_unwind(AssertUnwindSafe(|| {
        let _ = cas.put_file_with_hook(&src, &mut || panic!("power cut"));
    }));
    assert!(crashed.is_err(), "the hook must have fired");
    assert!(!cas.contains(&hash), "a crash must never leave a half object");

    assert_eq!(cas.sweep_tmp().unwrap(), 1, "the abandoned temp file is cleaned up");
    let again = cas.put_file(&src).unwrap();
    assert!(again.newly_stored);
    assert_eq!(fs::read(cas.path_of(&hash)).unwrap(), b"important bytes");
}

#[test]
fn an_object_written_just_before_a_crash_is_swept_when_the_vault_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    let cas = Cas::open(&vault).unwrap();
    let src = dir.path().join("a.docx");
    fs::write(&src, b"written, then the process died").unwrap();
    let stored = cas.put_file(&src).unwrap(); // the version row is never inserted
    assert!(cas.contains(&stored.hash));
    drop(cas);

    let _engine = Engine::open(&vault, Arc::new(SystemClock), Arc::new(RandomIdGen)).unwrap();
    assert!(!Cas::open(&vault).unwrap().contains(&stored.hash), "an object with no version is removed");
}
