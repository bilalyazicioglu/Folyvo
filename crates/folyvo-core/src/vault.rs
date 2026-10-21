use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::canonical::canonical_key;
use crate::cas::Cas;
use crate::error::{CoreError, Result};
use crate::ingest::{IngestOutcome, Ingestor};
use crate::model::Version;
use crate::ports::{Clock, IdGen};
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
}
