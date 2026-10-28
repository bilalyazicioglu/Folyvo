//! What changed in each save since the save before it, worked out in the background.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use folyvo_compare::{Outcome, Reading, Unit};
use folyvo_core::engine::{CompareInputs, Engine, EngineEvent, RecvError};
use folyvo_core::model::{ChangeCount, ChangeOutcome, ChangeRecord, ChangeUnit, ItemId, VersionId};

/// Works out a change record from the two stored files. Replaceable in tests.
pub type Summarise = Arc<dyn Fn(&CompareInputs) -> ChangeRecord + Send + Sync>;

/// What a file says, by its content and its name.
pub(crate) fn reading_of(path: &std::path::Path, name: &str) -> Reading {
    if folyvo_compare::is_pdf(path) {
        let limits = folyvo_compare::Limits::DEFAULT;
        return match crate::pdf::page_texts(path, &limits) {
            Ok(pages) => folyvo_compare::pdf_reading(pages, &limits),
            Err(why) => Reading::Unreadable(why),
        };
    }
    folyvo_compare::read(path, name)
}

/// Compares a save with the save before it. Only counts come out; the text is never kept.
pub fn summarise(inputs: &CompareInputs) -> ChangeRecord {
    let reader = folyvo_compare::READER_VERSION;
    let Some((before_id, before_path, before_hash)) = &inputs.before else {
        return ChangeRecord { compared_to: None, outcome: ChangeOutcome::First, counts: Vec::new(), reader };
    };
    let compared_to = Some(before_id.clone());
    if *before_hash == inputs.after_hash {
        return ChangeRecord { compared_to, outcome: ChangeOutcome::Identical, counts: Vec::new(), reader };
    }
    if !folyvo_compare::same_kind(before_path, &inputs.after, &inputs.name) {
        return ChangeRecord { compared_to, outcome: ChangeOutcome::Unknown, counts: Vec::new(), reader };
    }
    let changes = folyvo_compare::compare(
        &reading_of(before_path, &inputs.name),
        &reading_of(&inputs.after, &inputs.name),
    );
    let (outcome, counts) = match changes.outcome {
        Outcome::Text => (
            ChangeOutcome::Text,
            changes
                .counts
                .iter()
                .map(|c| ChangeCount {
                    unit: unit(c.unit),
                    added: c.added,
                    removed: c.removed,
                    changed: c.changed,
                })
                .collect(),
        ),
        Outcome::SameText => (ChangeOutcome::SameText, Vec::new()),
        Outcome::Unknown => (ChangeOutcome::Unknown, Vec::new()),
    };
    ChangeRecord { compared_to, outcome, counts, reader }
}

fn unit(u: Unit) -> ChangeUnit {
    match u {
        Unit::Line => ChangeUnit::Line,
        Unit::Paragraph => ChangeUnit::Paragraph,
        Unit::Cell => ChangeUnit::Cell,
        Unit::Sheet => ChangeUnit::Sheet,
        Unit::Page => ChangeUnit::Page,
    }
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<VersionId>,
    stop: bool,
}

type Shared = Arc<(Mutex<Queue>, Condvar)>;

/// One thread that compares saves one at a time: each new save as it arrives, and the saves of a
/// file whose versions window asks, ahead of the rest. Comparing happens outside the engine's
/// lock, so saving never waits for it.
pub struct ChangeWorker {
    queue: Shared,
    thread: Option<JoinHandle<()>>,
}

impl ChangeWorker {
    pub fn start(engine: Arc<Engine>) -> ChangeWorker {
        ChangeWorker::start_with(engine, Arc::new(summarise))
    }

    pub fn start_with(engine: Arc<Engine>, summarise: Summarise) -> ChangeWorker {
        let queue: Shared = Arc::default();
        let mut events = engine.subscribe();
        let heard = queue.clone();
        // Ends when the engine goes away and its event channel closes.
        std::thread::spawn(move || loop {
            match events.blocking_recv() {
                Ok(EngineEvent::VersionCreated { version, .. }) => push(&heard, vec![version], false),
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            }
        });
        let work = queue.clone();
        let thread = std::thread::spawn(move || {
            while let Some(version) = next(&work) {
                let Ok(inputs) = engine.compare_inputs(&version) else { continue };
                let record = summarise(&inputs);
                if let Err(e) = engine.store_change(&version, &record) {
                    eprintln!("folyvo: could not keep what changed in {version}: {e}");
                }
            }
        });
        ChangeWorker { queue, thread: Some(thread) }
    }

    /// Puts the saves of `item` that are not compared yet (or were compared by an older reader)
    /// at the front of the queue, newest first.
    pub fn backfill(&self, engine: &Engine, item: &ItemId) {
        if let Ok(versions) = engine.versions_needing_changes(item, folyvo_compare::READER_VERSION) {
            push(&self.queue, versions, true);
        }
    }
}

impl Drop for ChangeWorker {
    fn drop(&mut self) {
        let (lock, wake) = &*self.queue;
        lock.lock().unwrap().stop = true;
        wake.notify_all();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Adds `versions` once each, at the front (in the order given) or at the back.
fn push(queue: &Shared, versions: Vec<VersionId>, front: bool) {
    let (lock, wake) = &**queue;
    let mut q = lock.lock().unwrap();
    q.jobs.retain(|v| !versions.contains(v));
    if front {
        for v in versions.into_iter().rev() {
            q.jobs.push_front(v);
        }
    } else {
        q.jobs.extend(versions);
    }
    wake.notify_all();
}

/// The next job, waiting for one; `None` once the worker is stopping.
fn next(queue: &Shared) -> Option<VersionId> {
    let (lock, wake) = &**queue;
    let mut q = lock.lock().unwrap();
    loop {
        if q.stop {
            return None;
        }
        if let Some(v) = q.jobs.pop_front() {
            return Some(v);
        }
        q = wake.wait(q).unwrap();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use folyvo_core::engine::{CompareInputs, Engine};
    use folyvo_core::model::{ChangeCount, ChangeOutcome, ChangeUnit, ContentHash, ItemId, VersionId};
    use folyvo_core::ports::{RandomIdGen, SystemClock};
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    /// A Word file with one paragraph per string.
    pub(crate) fn docx(dir: &Path, name: &str, paras: &[&str]) -> PathBuf {
        let body: String = paras.iter().map(|p| format!("<w:p><w:r><w:t>{p}</w:t></w:r></w:p>")).collect();
        let xml = format!(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
        );
        let path = dir.join(name);
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("word/document.xml", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap();
        path
    }

    /// An Excel file with one sheet, `Costs`, holding `cells` as numbers.
    pub(crate) fn xlsx(dir: &Path, name: &str, cells: &[(&str, &str)]) -> PathBuf {
        let s = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
        let r = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
        let data: String =
            cells.iter().map(|(a, v)| format!(r#"<row><c r="{a}"><v>{v}</v></c></row>"#)).collect();
        let parts = [
            (
                "xl/workbook.xml",
                format!(
                    r#"<workbook xmlns="{s}" xmlns:r="{r}"><sheets><sheet name="Costs" sheetId="1" r:id="rId1"/></sheets></workbook>"#
                ),
            ),
            (
                "xl/_rels/workbook.xml.rels",
                format!(
                    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{r}/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#
                ),
            ),
            (
                "xl/worksheets/sheet1.xml",
                format!(r#"<worksheet xmlns="{s}"><sheetData>{data}</sheetData></worksheet>"#),
            ),
        ];
        let path = dir.join(name);
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (part, xml) in parts {
            zip.start_file(part, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(xml.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    fn file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn inputs(before: Option<&Path>, after: &Path, name: &str) -> CompareInputs {
        let hash = |p: &Path| ContentHash::of_bytes(&std::fs::read(p).unwrap());
        CompareInputs {
            item: ItemId::from_suffix("01"),
            version: VersionId::from_suffix("02"),
            after: after.to_path_buf(),
            after_hash: hash(after),
            before: before.map(|b| (VersionId::from_suffix("01"), b.to_path_buf(), hash(b))),
            name: name.into(),
        }
    }

    #[test]
    fn the_first_save_is_first() {
        let d = tempfile::tempdir().unwrap();
        let r = summarise(&inputs(None, &file(d.path(), "a", b"x"), "a.txt"));
        assert_eq!((r.outcome, r.compared_to, r.counts.len()), (ChangeOutcome::First, None, 0));
        assert_eq!(r.reader, folyvo_compare::READER_VERSION);
    }

    #[test]
    fn a_save_with_the_same_content_as_the_one_before_is_identical() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (file(d.path(), "a", b"same"), file(d.path(), "b", b"same"));
        let r = summarise(&inputs(Some(&a), &b, "a.txt"));
        assert_eq!(r.outcome, ChangeOutcome::Identical);
        assert_eq!(r.compared_to, Some(VersionId::from_suffix("01")));
    }

    #[test]
    fn two_word_saves_count_paragraphs() {
        let d = tempfile::tempdir().unwrap();
        let a = docx(d.path(), "a", &["Proposal", "Support, 12 months"]);
        let b = docx(d.path(), "b", &["Proposal", "Support, 18 months", "Valid for 30 days"]);
        let r = summarise(&inputs(Some(&a), &b, "Proposal.docx"));
        assert_eq!(r.outcome, ChangeOutcome::Text);
        assert_eq!(
            r.counts,
            vec![ChangeCount { unit: ChangeUnit::Paragraph, added: 1, removed: 0, changed: 1 }]
        );
    }

    #[test]
    fn different_kinds_are_unknown() {
        let d = tempfile::tempdir().unwrap();
        let a = docx(d.path(), "a", &["Proposal"]);
        let b = file(d.path(), "b", b"Proposal");
        assert_eq!(summarise(&inputs(Some(&a), &b, "Proposal.docx")).outcome, ChangeOutcome::Unknown);
    }

    #[test]
    fn a_word_file_replaced_by_plain_text_is_unknown() {
        let d = tempfile::tempdir().unwrap();
        let a = docx(d.path(), "a", &["Proposal"]);
        let b = file(d.path(), "b", b"Proposal, rewritten");
        assert_eq!(summarise(&inputs(Some(&a), &b, "Proposal.txt")).outcome, ChangeOutcome::Unknown);
    }

    #[test]
    fn same_text_when_only_bytes_differ() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (file(d.path(), "a", b"a\n"), file(d.path(), "b", b"a\n\n"));
        assert_eq!(summarise(&inputs(Some(&a), &b, "n.txt")).outcome, ChangeOutcome::SameText);
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        engine: Arc<Engine>,
        work: PathBuf,
    }

    fn engine() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let engine = Arc::new(
            Engine::open(&dir.path().join("vault"), Arc::new(SystemClock), Arc::new(RandomIdGen)).unwrap(),
        );
        let work = dir.path().join("work");
        std::fs::create_dir(&work).unwrap();
        engine.add_root(&work).unwrap();
        Fixture { _dir: dir, engine, work }
    }

    fn save(f: &Fixture, name: &str, bytes: &[u8]) -> folyvo_core::Version {
        let p = f.work.join(name);
        std::fs::write(&p, bytes).unwrap();
        f.engine.ingest_path(&p).unwrap().unwrap().version().clone()
    }

    fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if f() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("timed out waiting for {what}");
    }

    #[test]
    fn the_worker_fills_in_a_new_save() {
        let f = engine();
        let _worker = ChangeWorker::start(f.engine.clone());
        let v1 = save(&f, "n.txt", b"one\n");
        save(&f, "n.txt", b"one\n\ntwo\n");
        wait_until("both saves to be compared", || {
            let h = f.engine.history(&v1.item_id).unwrap();
            h.iter().all(|v| v.change.is_some())
        });
        let h = f.engine.history(&v1.item_id).unwrap();
        assert_eq!(h[0].change.as_ref().unwrap().outcome, ChangeOutcome::Text);
        assert_eq!(h[1].change.as_ref().unwrap().outcome, ChangeOutcome::First);
    }

    #[test]
    fn the_engine_saves_while_a_comparison_runs() {
        let f = engine();
        let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = started.clone();
        let slow: Summarise = Arc::new(move |i: &CompareInputs| {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            std::thread::sleep(Duration::from_secs(2));
            summarise(i)
        });
        let _worker = ChangeWorker::start_with(f.engine.clone(), slow);
        save(&f, "n.txt", b"one");
        wait_until("the slow comparison to start", || started.load(std::sync::atomic::Ordering::SeqCst));
        let at = Instant::now();
        save(&f, "n.txt", b"two");
        assert!(at.elapsed() < Duration::from_secs(1), "saving waited {:?}", at.elapsed());
    }

    #[test]
    fn backfill_puts_the_asked_item_first() {
        let f = engine();
        let busy = save(&f, "busy.txt", b"x");
        let a = save(&f, "a.txt", b"a");
        let b = save(&f, "b.txt", b"b");
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = order.clone();
        let record: Summarise = Arc::new(move |i: &CompareInputs| {
            seen.lock().unwrap().push(i.item.clone());
            if seen.lock().unwrap().len() == 1 {
                std::thread::sleep(Duration::from_millis(300)); // keep the worker busy while more is asked
            }
            summarise(i)
        });
        let worker = ChangeWorker::start_with(f.engine.clone(), record);
        worker.backfill(&f.engine, &busy.item_id);
        wait_until("the first job to start", || order.lock().unwrap().len() == 1);
        worker.backfill(&f.engine, &a.item_id);
        worker.backfill(&f.engine, &b.item_id);
        wait_until("all three to be compared", || order.lock().unwrap().len() == 3);
        assert_eq!(order.lock().unwrap()[1..], [b.item_id.clone(), a.item_id.clone()]);
    }
}
