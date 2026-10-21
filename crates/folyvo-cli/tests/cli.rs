use std::fs;
use std::process::Command;

fn folyvo(vault: &std::path::Path, args: &[&str]) -> (bool, String) {
    let out =
        Command::new(env!("CARGO_BIN_EXE_folyvo")).env("FOLYVO_VAULT", vault).args(args).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    (out.status.success(), text)
}

#[test]
fn save_then_log_shows_layers_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    let f = dir.path().join("Proposal.docx");

    fs::write(&f, b"first draft").unwrap();
    let (ok, text) = folyvo(&vault, &["save", f.to_str().unwrap()]);
    assert!(ok, "{text}");
    assert!(text.contains("Saved v1"), "{text}");

    fs::write(&f, b"second draft, longer").unwrap();
    let (ok, text) = folyvo(&vault, &["save", f.to_str().unwrap()]);
    assert!(ok && text.contains("Saved v2"), "{text}");

    let (ok, text) = folyvo(&vault, &["log", f.to_str().unwrap()]);
    assert!(ok, "{text}");
    let v2 = text.find("v2").unwrap();
    let v1 = text.find("v1").unwrap();
    assert!(v2 < v1, "newest layer must come first:\n{text}");
    assert!(text.contains("Proposal.docx  2 versions"), "{text}");
}

#[test]
fn saving_unchanged_content_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    let f = dir.path().join("a.txt");
    fs::write(&f, b"same").unwrap();
    folyvo(&vault, &["save", f.to_str().unwrap()]);
    let (ok, text) = folyvo(&vault, &["save", f.to_str().unwrap()]);
    assert!(ok && text.contains("Nothing changed since v1"), "{text}");
}

#[test]
fn log_of_an_unknown_file_fails_with_a_plain_message() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("never.txt");
    fs::write(&f, b"x").unwrap();
    let (ok, text) = folyvo(&dir.path().join("vault"), &["log", f.to_str().unwrap()]);
    assert!(!ok);
    assert!(text.contains("has no saved versions"), "{text}");
}

#[test]
fn ready_export_and_restore_work_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    let out = dir.path().join("out");
    fs::create_dir(&out).unwrap();
    let f = dir.path().join("Proposal.docx");
    let p = f.to_str().unwrap();

    fs::write(&f, b"sent to the client").unwrap();
    folyvo(&vault, &["save", p]);
    fs::write(&f, b"edited afterwards").unwrap();
    folyvo(&vault, &["save", p]);

    let (ok, text) = folyvo(&vault, &["ready", p, "v1"]);
    assert!(ok && text.contains("v1 is ready to share"), "{text}");

    let (_, log) = folyvo(&vault, &["log", p]);
    let ready_line = log.lines().find(|l| l.contains("ready to share")).expect(&log);
    assert!(ready_line.starts_with("v1"), "{log}");
    assert!(log.contains("Changed since v1"), "{log}");

    let (ok, text) = folyvo(&vault, &["export", p, out.to_str().unwrap()]);
    assert!(ok && text.contains("Exported v1 to"), "{text}");
    assert_eq!(fs::read(out.join("Proposal.docx")).unwrap(), b"sent to the client");

    let (ok, text) = folyvo(&vault, &["restore", p, "v1"]);
    assert!(ok && text.contains("Restored v1 as v3"), "{text}");
    assert_eq!(fs::read(&f).unwrap(), b"sent to the client");
}

#[test]
fn unknown_version_and_missing_ready_give_plain_messages() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    let f = dir.path().join("a.txt");
    fs::write(&f, b"x").unwrap();
    let p = f.to_str().unwrap();
    folyvo(&vault, &["save", p]);

    let (ok, text) = folyvo(&vault, &["restore", p, "v9"]);
    assert!(!ok && text.contains("no version v9"), "{text}");
    let (ok, text) = folyvo(&vault, &["export", p, dir.path().to_str().unwrap()]);
    assert!(!ok && text.contains("no share-ready version"), "{text}");
    let (ok, text) = folyvo(&vault, &["restore", p, "latest"]);
    assert!(!ok && text.contains("version like v2"), "{text}");
}
