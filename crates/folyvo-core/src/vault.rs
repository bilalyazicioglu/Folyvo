use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::canonical::canonical_key;
use crate::cas::Cas;
use crate::error::{CoreError, Result};
use crate::events::share_events;
use crate::export::{ExportResult, Exporter};
use crate::ingest::{IngestOutcome, Ingestor};
use crate::lock::VaultLock;
use crate::model::{version_codes, ItemId, ShareEvent, Version, VersionId};
use crate::ports::{Clock, IdGen};
use crate::restore::Restorer;
use crate::store::{repo, Store};
use crate::which::{self, Match};

/// Shortest code prefix a person may type.
const MIN_CODE: usize = 4;

pub struct History {
    pub name: String,
    /// Newest first.
    pub versions: Vec<Version>,
    /// Short code of each version, same order as `versions`.
    pub codes: Vec<String>,
    /// Copies saved or handed out, oldest first.
    pub events: Vec<ShareEvent>,
}

pub struct Vault {
    /// Keeps the desktop app from sweeping objects this process is still writing.
    _lock: VaultLock,
    store: Store,
    cas: Cas,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
}

impl Vault {
    pub fn open(dir: &Path, clock: Arc<dyn Clock>, ids: Arc<dyn IdGen>) -> Result<Vault> {
        fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
        let cas = Cas::open(dir)?;
        let (lock, _) = VaultLock::acquire(dir)?;
        lock.share()?;
        let store = Store::open(&dir.join("folyvo.db"))?;
        Ok(Vault { store, cas, clock, ids, _lock: lock })
    }

    pub fn save(&mut self, path: &Path) -> Result<IngestOutcome> {
        Ingestor::new(&mut self.store, &self.cas, self.clock.as_ref(), self.ids.as_ref()).ingest(path)
    }

    pub fn history(&self, path: &Path) -> Result<History> {
        let key = canonical_key(path)?;
        let c = self.store.conn();
        let item = repo::item_at_key(c, &key)?.ok_or(CoreError::ItemMissing)?;
        let versions = repo::versions_newest_first(c, &item)?;
        let codes = version_codes(&versions.iter().map(|v| v.id.clone()).collect::<Vec<_>>());
        let events = share_events(&self.store, &item)?;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(History { name, versions, codes, events })
    }

    /// Bring a version (see [`Vault::resolve`]) back as a new version.
    pub fn restore(&mut self, path: &Path, selector: &str) -> Result<Version> {
        let item = self.item_of(path)?;
        let version = self.resolve(path, selector)?;
        Restorer::new(&mut self.store, &self.cas, self.clock.as_ref(), self.ids.as_ref())
            .restore(&item, &version)
    }

    /// Copy a version (the newest when `selector` is `None`) into `dest_dir`.
    pub fn copy(&mut self, path: &Path, selector: Option<&str>, dest_dir: &Path) -> Result<ExportResult> {
        let item = self.item_of(path)?;
        let version = match selector {
            Some(s) => self.resolve(path, s)?,
            None => repo::latest_version(self.store.conn(), &item)?.ok_or(CoreError::ItemMissing)?.id,
        };
        Exporter::new(&mut self.store, &self.cas, self.clock.as_ref()).export(&item, &version, dest_dir)
    }

    /// Every saved version whose content is exactly the file at `path`.
    pub fn which(&self, path: &Path) -> Result<Vec<Match>> {
        which::which_one(&self.store, path)
    }

    /// Name a version of the file: `vN` or a number under four digits is a sequence number,
    /// anything else is the start of a version code (at least four characters).
    pub fn resolve(&self, path: &Path, selector: &str) -> Result<VersionId> {
        let item = self.item_of(path)?;
        let digits = selector.strip_prefix('v').unwrap_or(selector);
        let is_seq = selector.starts_with('v') || selector.len() < MIN_CODE;
        if is_seq && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            let seq: i64 = digits.parse().map_err(|_| CoreError::NoSuchVersion(selector.to_owned()))?;
            return self.version_by_seq(&item, seq);
        }
        if selector.len() < MIN_CODE {
            return Err(CoreError::NoSuchVersion(selector.to_owned()));
        }
        let hits: Vec<VersionId> = repo::versions_newest_first(self.store.conn(), &item)?
            .into_iter()
            .map(|v| v.id)
            .filter(|id| id.as_str().strip_prefix(VersionId::PREFIX).is_some_and(|s| s.starts_with(selector)))
            .collect();
        match hits.as_slice() {
            [one] => Ok(one.clone()),
            [] => Err(CoreError::NoSuchVersion(selector.to_owned())),
            _ => Err(CoreError::AmbiguousVersion(selector.to_owned())),
        }
    }

    fn item_of(&self, path: &Path) -> Result<ItemId> {
        let key = canonical_key(path)?;
        repo::item_at_key(self.store.conn(), &key)?.ok_or(CoreError::ItemMissing)
    }

    fn version_by_seq(&self, item: &ItemId, seq: i64) -> Result<VersionId> {
        repo::versions_newest_first(self.store.conn(), item)?
            .into_iter()
            .find(|v| v.seq == seq)
            .map(|v| v.id)
            .ok_or(CoreError::NoSuchVersionNumber(seq))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreError;
    use crate::ports::{FakeClock, SeqIdGen};
    use std::fs;

    fn vault(dir: &std::path::Path) -> Vault {
        Vault::open(&dir.join("vault"), Arc::new(FakeClock::new(1_000)), Arc::new(SeqIdGen::new())).unwrap()
    }

    #[test]
    fn saving_twice_with_a_change_gives_two_versions_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut v = vault(dir.path());
        let f = dir.path().join("Proposal.docx");
        fs::write(&f, b"one").unwrap();
        v.save(&f).unwrap();
        fs::write(&f, b"two!").unwrap();
        v.save(&f).unwrap();

        let h = v.history(&f).unwrap();
        assert_eq!(h.name, "Proposal.docx");
        let seqs: Vec<i64> = h.versions.iter().map(|x| x.seq).collect();
        assert_eq!(seqs, vec![2, 1]);
        assert_eq!(h.versions[0].size, 4);
    }

    #[test]
    fn saving_unchanged_content_adds_no_version() {
        let dir = tempfile::tempdir().unwrap();
        let mut v = vault(dir.path());
        let f = dir.path().join("a.txt");
        fs::write(&f, b"same").unwrap();
        v.save(&f).unwrap();
        v.save(&f).unwrap();
        assert_eq!(v.history(&f).unwrap().versions.len(), 1);
    }

    #[test]
    fn history_of_a_file_never_saved_is_item_missing() {
        let dir = tempfile::tempdir().unwrap();
        let v = vault(dir.path());
        let f = dir.path().join("never.txt");
        fs::write(&f, b"x").unwrap();
        assert!(matches!(v.history(&f), Err(CoreError::ItemMissing)));
    }

    #[test]
    fn a_vault_reopened_on_the_same_directory_remembers_versions() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        fs::write(&f, b"x").unwrap();
        vault(dir.path()).save(&f).unwrap();
        assert_eq!(vault(dir.path()).history(&f).unwrap().versions.len(), 1);
    }

    #[test]
    fn copying_and_restoring_round_trip_through_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        let mut v = vault(dir.path());
        let f = dir.path().join("Proposal.docx");
        fs::write(&f, b"sent to the client").unwrap();
        v.save(&f).unwrap();
        fs::write(&f, b"edited afterwards").unwrap();
        v.save(&f).unwrap();

        let out = dir.path().join("out");
        fs::create_dir(&out).unwrap();
        let exported = v.copy(&f, None, &out).unwrap();
        assert_eq!(fs::read(&exported.destination).unwrap(), b"edited afterwards");

        let restored = v.restore(&f, "v1").unwrap();
        assert_eq!(restored.seq, 3);
        assert_eq!(fs::read(&f).unwrap(), b"sent to the client");
        assert_eq!(v.history(&f).unwrap().versions.len(), 3, "nothing was lost");
    }

    #[test]
    fn unknown_version_numbers_are_reported_not_guessed() {
        let dir = tempfile::tempdir().unwrap();
        let mut v = vault(dir.path());
        let f = dir.path().join("a.txt");
        fs::write(&f, b"x").unwrap();
        v.save(&f).unwrap();
        assert!(v.restore(&f, "v9").is_err());
    }

    /// Hands out the given suffixes in order (first the item, then each version).
    struct ListIds(std::sync::Mutex<std::collections::VecDeque<String>>);
    impl IdGen for ListIds {
        fn next_suffix(&self) -> String {
            self.0.lock().unwrap().pop_front().expect("test ran out of ids")
        }
    }

    fn vault_with_ids(dir: &std::path::Path, ids: &[&str]) -> Vault {
        let ids = ListIds(std::sync::Mutex::new(ids.iter().map(|s| s.to_string()).collect()));
        Vault::open(&dir.join("vault"), Arc::new(crate::ports::FakeClock::new(1)), Arc::new(ids)).unwrap()
    }

    fn two_saves(dir: &std::path::Path, v: &mut Vault) -> std::path::PathBuf {
        let f = dir.join("a.docx");
        fs::write(&f, b"one").unwrap();
        v.save(&f).unwrap();
        fs::write(&f, b"two").unwrap();
        v.save(&f).unwrap();
        f
    }

    #[test]
    fn resolve_refuses_an_ambiguous_code() {
        let dir = tempfile::tempdir().unwrap();
        let mut v = vault_with_ids(dir.path(), &["00", "abcd1111", "abcd2222"]);
        let f = two_saves(dir.path(), &mut v);
        assert!(matches!(v.resolve(&f, "abcd"), Err(CoreError::AmbiguousVersion(_))));
    }

    #[test]
    fn resolve_accepts_a_unique_prefix_and_v_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let mut v = vault_with_ids(dir.path(), &["00", "abcd1111", "abcd2222"]);
        let f = two_saves(dir.path(), &mut v);
        assert_eq!(v.resolve(&f, "abcd1").unwrap().as_str(), "ver_abcd1111");
        assert_eq!(v.resolve(&f, "v2").unwrap().as_str(), "ver_abcd2222");
        assert_eq!(v.resolve(&f, "2").unwrap().as_str(), "ver_abcd2222");
    }

    #[test]
    fn resolve_rejects_short_or_unknown_codes() {
        let dir = tempfile::tempdir().unwrap();
        let mut v = vault_with_ids(dir.path(), &["00", "abcd1111", "abcd2222"]);
        let f = two_saves(dir.path(), &mut v);
        for bad in ["abc", "ffff", "zzzz1"] {
            assert!(matches!(v.resolve(&f, bad), Err(CoreError::NoSuchVersion(_))), "{bad}");
        }
        assert!(matches!(v.resolve(&f, "v9"), Err(CoreError::NoSuchVersionNumber(9))));
    }

    #[test]
    fn copy_takes_the_newest_or_the_version_asked_for() {
        let dir = tempfile::tempdir().unwrap();
        let mut v = vault_with_ids(dir.path(), &["00", "abcd1111", "abcd2222"]);
        let f = two_saves(dir.path(), &mut v);
        let out = dir.path().join("out");
        fs::create_dir(&out).unwrap();
        let newest = v.copy(&f, None, &out).unwrap();
        assert_eq!(fs::read(&newest.destination).unwrap(), b"two");
        let older = v.copy(&f, Some("abcd1"), &out).unwrap();
        assert_eq!(fs::read(&older.destination).unwrap(), b"one");
    }

    #[test]
    fn history_carries_codes_and_copies() {
        let dir = tempfile::tempdir().unwrap();
        let mut v = vault_with_ids(dir.path(), &["00", "abcd1111", "abcd2222"]);
        let f = two_saves(dir.path(), &mut v);
        let out = dir.path().join("out");
        fs::create_dir(&out).unwrap();
        v.copy(&f, Some("v1"), &out).unwrap();
        let h = v.history(&f).unwrap();
        assert_eq!(h.codes, vec!["abcd2222", "abcd1111"]);
        assert_eq!(h.events.len(), 1);
    }

    #[test]
    fn which_finds_a_copy() {
        let dir = tempfile::tempdir().unwrap();
        let mut v = vault_with_ids(dir.path(), &["00", "abcd1111", "abcd2222"]);
        let f = two_saves(dir.path(), &mut v);
        let copy = dir.path().join("copy.docx");
        fs::write(&copy, b"one").unwrap();
        let m = v.which(&copy).unwrap();
        assert_eq!(m.iter().map(|m| m.version.code.clone()).collect::<Vec<_>>(), vec!["abcd1111"]);
        let _ = f;
    }
}
