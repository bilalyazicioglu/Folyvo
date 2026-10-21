use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

struct Watching {
    child: Child,
    lines: Receiver<String>,
    seen: Vec<String>,
}

impl Watching {
    fn start(vault: &std::path::Path, folder: &std::path::Path) -> Watching {
        let mut child = Command::new(env!("CARGO_BIN_EXE_folyvo"))
            .env("FOLYVO_VAULT", vault)
            .env("FOLYVO_QUIET_MS", "150,150")
            .args(["watch", folder.to_str().unwrap()])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let out = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Watching { child, lines: rx, seen: vec![] }
    }

    fn wait_for(&mut self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if let Ok(line) = self.lines.recv_timeout(Duration::from_millis(200)) {
                self.seen.push(line.clone());
                if line.contains(needle) {
                    return;
                }
            }
        }
        panic!("never saw {needle:?}; output so far: {:#?}", self.seen);
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn saving_in_a_watched_folder_creates_layers_without_any_command() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Client work");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("existing.txt"), b"was here before").unwrap();
    let mut w = Watching::start(&dir.path().join("vault"), &folder);

    w.wait_for("Saved existing.txt as v1"); // found by the startup scan
    w.wait_for("Watching"); // printed only once the watcher is live, so nothing below can be missed

    let f = folder.join("Proposal.docx");
    fs::write(&f, b"first draft").unwrap();
    w.wait_for("Saved Proposal.docx as v1");

    fs::write(&f, b"second draft, longer").unwrap();
    w.wait_for("Saved Proposal.docx as v2");

    fs::write(folder.join("~$Proposal.docx"), b"office lock file").unwrap();
    fs::remove_file(&f).unwrap();
    w.wait_for("Proposal.docx is missing");
    assert!(!w.seen.iter().any(|l| l.contains("~$")), "{:#?}", w.seen);
}
