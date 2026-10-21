use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const KNOWN_QUIET_MS: i64 = 2_000;
pub const NEW_QUIET_MS: i64 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsEvent {
    Changed,
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Ingest,
    MarkMissing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Due {
    pub path: PathBuf,
    pub action: Action,
}

struct Pending {
    due_at_ms: i64,
    action: Action,
}

pub struct Debouncer {
    known_quiet_ms: i64,
    new_quiet_ms: i64,
    pending: BTreeMap<PathBuf, Pending>,
}

impl Debouncer {
    pub fn new(known_quiet_ms: i64, new_quiet_ms: i64) -> Self {
        Self { known_quiet_ms, new_quiet_ms, pending: BTreeMap::new() }
    }

    pub fn record(&mut self, path: &Path, event: FsEvent, now_ms: i64, known: bool) {
        let quiet = if known { self.known_quiet_ms } else { self.new_quiet_ms };
        let action = match event {
            FsEvent::Changed => Action::Ingest,
            FsEvent::Removed => Action::MarkMissing,
        };
        self.pending.insert(path.to_path_buf(), Pending { due_at_ms: now_ms + quiet, action });
    }

    pub fn due(&mut self, now_ms: i64) -> Vec<Due> {
        let ready: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, p)| p.due_at_ms <= now_ms)
            .map(|(path, _)| path.clone())
            .collect();
        ready
            .into_iter()
            .filter_map(|path| self.pending.remove(&path).map(|p| Due { path, action: p.action }))
            .collect()
    }

    pub fn next_deadline(&self) -> Option<i64> {
        self.pending.values().map(|p| p.due_at_ms).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn d() -> Debouncer {
        Debouncer::new(2_000, 5_000)
    }

    fn due(path: &str, action: Action) -> Due {
        Due { path: PathBuf::from(path), action }
    }

    #[test]
    fn a_change_is_due_only_after_the_quiet_period() {
        let mut db = d();
        db.record(Path::new("/w/a.docx"), FsEvent::Changed, 1_000, true);
        assert!(db.due(2_999).is_empty());
        assert_eq!(db.due(3_000), vec![due("/w/a.docx", Action::Ingest)]);
        assert!(db.due(10_000).is_empty(), "a path is reported once");
    }

    #[test]
    fn a_burst_of_events_restarts_the_timer_and_yields_one_due() {
        let mut db = d();
        for t in [0, 500, 1_000, 1_500] {
            db.record(Path::new("/w/a.docx"), FsEvent::Changed, t, true);
        }
        assert!(db.due(3_000).is_empty());
        assert_eq!(db.due(3_500).len(), 1);
    }

    #[test]
    fn removed_then_changed_is_an_ingest_the_excel_pattern() {
        let mut db = d();
        db.record(Path::new("/w/a.xlsx"), FsEvent::Removed, 0, true);
        db.record(Path::new("/w/a.xlsx"), FsEvent::Changed, 400, true);
        assert_eq!(db.due(2_400), vec![due("/w/a.xlsx", Action::Ingest)]);
    }

    #[test]
    fn a_removal_nobody_undoes_marks_the_file_missing() {
        let mut db = d();
        db.record(Path::new("/w/a.docx"), FsEvent::Removed, 0, true);
        assert!(db.due(1_999).is_empty());
        assert_eq!(db.due(2_000), vec![due("/w/a.docx", Action::MarkMissing)]);
    }

    #[test]
    fn changed_then_removed_ends_as_missing() {
        let mut db = d();
        db.record(Path::new("/w/a.docx"), FsEvent::Changed, 0, true);
        db.record(Path::new("/w/a.docx"), FsEvent::Removed, 100, true);
        assert_eq!(db.due(2_100), vec![due("/w/a.docx", Action::MarkMissing)]);
    }

    #[test]
    fn a_path_never_seen_before_waits_longer() {
        let mut db = d();
        db.record(Path::new("/w/new.docx"), FsEvent::Changed, 0, false);
        assert!(db.due(4_999).is_empty());
        assert_eq!(db.due(5_000).len(), 1);
    }

    #[test]
    fn paths_are_independent_and_returned_in_path_order() {
        let mut db = d();
        db.record(Path::new("/w/b.docx"), FsEvent::Changed, 0, true);
        db.record(Path::new("/w/a.docx"), FsEvent::Changed, 1_000, true);
        assert_eq!(db.due(2_000), vec![due("/w/b.docx", Action::Ingest)]);
        assert_eq!(db.due(3_000), vec![due("/w/a.docx", Action::Ingest)]);
    }

    #[test]
    fn next_deadline_is_the_earliest_pending_due_time() {
        let mut db = d();
        assert_eq!(db.next_deadline(), None);
        db.record(Path::new("/w/a.docx"), FsEvent::Changed, 0, false); // due 5000
        db.record(Path::new("/w/b.docx"), FsEvent::Changed, 1_000, true); // due 3000
        assert_eq!(db.next_deadline(), Some(3_000));
        db.due(3_000);
        assert_eq!(db.next_deadline(), Some(5_000));
    }

    #[test]
    fn default_constants_match_the_spec() {
        assert_eq!(KNOWN_QUIET_MS, 2_000);
        const { assert!(NEW_QUIET_MS > KNOWN_QUIET_MS) };
    }
}
