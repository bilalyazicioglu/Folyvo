use std::fs;
use std::process::Command;

fn folyvo(vault: &std::path::Path, args: &[&str]) -> (bool, String) {
    let out =
        Command::new(env!("CARGO_BIN_EXE_folyvo")).env("FOLYVO_VAULT", vault).args(args).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    (out.status.success(), text)
}

#[test]
fn save_then_log_lists_versions_newest_first_with_codes() {
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
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "Proposal.docx  2 versions", "{text}");
    assert_eq!(lines.len(), 3, "{text}");
    let newest = code_of(lines[1]);
    let first = code_of(lines[2]);
    assert!(lines[2].ends_with("First version"), "the oldest comes last:\n{text}");
    assert_ne!(newest, first);
    assert!(!text.contains("v1") && !text.contains('█'), "{text}");
}

/// The second field of a log line is the version code: 8 lowercase hex characters.
fn code_of(line: &str) -> String {
    let code = line.split("  ").nth(1).unwrap_or_default();
    assert!(
        code.len() == 8 && code.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "{line:?}"
    );
    code.to_string()
}

fn two_drafts(dir: &std::path::Path, vault: &std::path::Path) -> std::path::PathBuf {
    let f = dir.join("Proposal.docx");
    fs::write(&f, b"sent to the client").unwrap();
    folyvo(vault, &["save", f.to_str().unwrap()]);
    fs::write(&f, b"edited afterwards").unwrap();
    folyvo(vault, &["save", f.to_str().unwrap()]);
    f
}

#[test]
fn copy_without_a_version_copies_the_newest() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    let f = two_drafts(dir.path(), &vault);
    let out = dir.path().join("out");
    fs::create_dir(&out).unwrap();
    let (ok, text) = folyvo(&vault, &["copy", f.to_str().unwrap(), out.to_str().unwrap()]);
    assert!(ok && text.starts_with("Saved a copy of "), "{text}");
    assert_eq!(fs::read(out.join("Proposal.docx")).unwrap(), b"edited afterwards");
}

#[test]
fn copy_takes_a_code_or_a_v_number() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    let f = two_drafts(dir.path(), &vault);
    let p = f.to_str().unwrap();
    let (_, log) = folyvo(&vault, &["log", p]);
    let first = code_of(log.lines().nth(2).unwrap());
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    let (ok, text) = folyvo(&vault, &["copy", p, a.to_str().unwrap(), &first[..5]]);
    assert!(ok, "{text}");
    assert_eq!(fs::read(a.join("Proposal.docx")).unwrap(), b"sent to the client");
    let (ok, text) = folyvo(&vault, &["copy", p, b.to_str().unwrap(), "v1"]);
    assert!(ok, "{text}");
    assert_eq!(fs::read(b.join("Proposal.docx")).unwrap(), b"sent to the client");
    let (_, log) = folyvo(&vault, &["log", p]);
    assert!(log.contains("  Copy saved to "), "{log}");
}

#[test]
fn which_names_the_saved_time_and_code() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    let f = two_drafts(dir.path(), &vault);
    let (_, log) = folyvo(&vault, &["log", f.to_str().unwrap()]);
    let first = code_of(log.lines().nth(2).unwrap());
    let back = dir.path().join("from-the-client.docx");
    fs::write(&back, b"sent to the client").unwrap();
    let (ok, text) = folyvo(&vault, &["which", back.to_str().unwrap()]);
    assert!(ok, "{text}");
    assert!(
        text.starts_with("Proposal.docx, ") && text.trim_end().ends_with(&format!("({first})")),
        "{text}"
    );

    fs::write(&back, b"sent to the client, then edited").unwrap();
    let (ok, text) = folyvo(&vault, &["which", back.to_str().unwrap()]);
    assert!(ok, "{text}");
    assert_eq!(
        text.trim_end(),
        "No saved version has exactly this content. It may have been opened and saved again since it left."
    );
}

#[test]
fn export_is_now_called_copy() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    let f = two_drafts(dir.path(), &vault);
    let (ok, text) = folyvo(&vault, &["export", f.to_str().unwrap(), dir.path().to_str().unwrap()]);
    assert!(!ok && text.contains("usage:") && text.contains("copy <file> <folder> [version]"), "{text}");
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
fn copy_and_restore_work_end_to_end() {
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

    let (_, log) = folyvo(&vault, &["log", p]);
    assert!(!log.contains("ready"), "{log}");

    let (ok, text) = folyvo(&vault, &["copy", p, out.to_str().unwrap()]);
    assert!(ok && text.contains("Saved a copy of"), "{text}");
    assert_eq!(fs::read(out.join("Proposal.docx")).unwrap(), b"edited afterwards");

    let (ok, text) = folyvo(&vault, &["restore", p, "v1"]);
    assert!(ok && text.contains("Restored v1 as v3"), "{text}");
    assert_eq!(fs::read(&f).unwrap(), b"sent to the client");
}

#[test]
fn unknown_versions_give_plain_messages() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    let f = dir.path().join("a.txt");
    fs::write(&f, b"x").unwrap();
    let p = f.to_str().unwrap();
    folyvo(&vault, &["save", p]);

    let (ok, text) = folyvo(&vault, &["restore", p, "v9"]);
    assert!(!ok && text.contains("no version v9"), "{text}");
    let (ok, text) = folyvo(&vault, &["restore", p, "latest"]);
    assert!(!ok && text.contains("\"latest\" is not a version of this file"), "{text}");
}

#[test]
fn ready_is_no_longer_a_command() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    let f = dir.path().join("a.txt");
    fs::write(&f, b"x").unwrap();
    let p = f.to_str().unwrap();
    folyvo(&vault, &["save", p]);
    let (ok, text) = folyvo(&vault, &["ready", p, "v1"]);
    assert!(!ok && text.contains("usage:") && !text.contains("ready"), "{text}");
}

// Final review, Important 2: the CLI used to keep its history in ~/.folyvo.
#[test]
fn an_old_vault_in_the_home_folder_is_pointed_out_not_silently_ignored() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir(home.path().join(".folyvo")).unwrap();
    fs::write(home.path().join(".folyvo").join("folyvo.db"), b"").unwrap();
    let f = home.path().join("a.txt");
    fs::write(&f, b"x").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_folyvo"))
        .env_remove("FOLYVO_VAULT")
        .env_remove("XDG_DATA_HOME")
        .env("HOME", home.path())
        .args(["log", f.to_str().unwrap()])
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("~/.folyvo") && err.contains("FOLYVO_VAULT"), "{err}");
}
