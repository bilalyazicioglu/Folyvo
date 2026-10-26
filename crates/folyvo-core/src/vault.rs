use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::canonical::canonical_key;
use crate::cas::Cas;
use crate::error::{CoreError, Result};
use crate::export::{ExportResult, Exporter};
use crate::ingest::{IngestOutcome, Ingestor};
use crate::model::{ItemId, Version, VersionId};
use crate::ports::{Clock, IdGen};
use crate::restore::Restorer;
use crate::store::{repo, Store};

pub struct History {
    pub name: String,
    pub versions: Vec<Version>,
}

pub struct Vault {
    store: Store,
    cas: Cas,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
}

impl Vault {
    pub fn open(dir: &Path, clock: Arc<dyn Clock>, ids: Arc<dyn IdGen>) -> Result<Vault> {
        fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
        let cas = Cas::open(dir)?;
        let store = Store::open(&dir.join("folyvo.db"))?;
        Ok(Vault { store, cas, clock, ids })
    }

    pub fn save(&mut self, path: &Path) -> Result<IngestOutcome> {
        Ingestor::new(&mut self.store, &self.cas, self.clock.as_ref(), self.ids.as_ref()).ingest(path)
    }

    pub fn history(&self, path: &Path) -> Result<History> {
        let key = canonical_key(path)?;
        let c = self.store.conn();
        let item = repo::item_at_key(c, &key)?.ok_or(CoreError::ItemMissing)?;
        let versions = repo::versions_newest_first(c, &item)?;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(History { name, versions })
    }

    /// Copy the newest version into `dest_dir`; the original is never touched.
    pub fn export(&mut self, path: &Path, dest_dir: &Path) -> Result<ExportResult> {
        let item = self.item_of(path)?;
        let newest = repo::latest_version(self.store.conn(), &item)?.ok_or(CoreError::ItemMissing)?;
        Exporter::new(&mut self.store, &self.cas, self.clock.as_ref()).export(&item, &newest.id, dest_dir)
    }

    /// Bring version number `seq` back as a new version.
    pub fn restore(&mut self, path: &Path, seq: i64) -> Result<Version> {
        let item = self.item_of(path)?;
        let version = self.version_by_seq(&item, seq)?;
        Restorer::new(&mut self.store, &self.cas, self.clock.as_ref(), self.ids.as_ref())
            .restore(&item, &version)
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
        let exported = v.export(&f, &out).unwrap();
        assert_eq!(fs::read(&exported.destination).unwrap(), b"edited afterwards");

        let restored = v.restore(&f, 1).unwrap();
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
        assert!(v.restore(&f, 9).is_err());
    }
}
