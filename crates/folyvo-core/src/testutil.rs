use std::fs;
use std::path::{Path, PathBuf};

use crate::cas::Cas;
use crate::error::Result;
use crate::ingest::{IngestOutcome, Ingestor};
use crate::ports::{FakeClock, SeqIdGen};
use crate::store::Store;

pub(crate) struct Env {
    pub dir: tempfile::TempDir,
    pub store: Store,
    pub cas: Cas,
    pub clock: FakeClock,
    pub ids: SeqIdGen,
}

impl Env {
    pub fn new() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::open(&dir.path().join("vault")).unwrap();
        fs::create_dir(dir.path().join("work")).unwrap();
        Env {
            dir,
            store: Store::open_in_memory().unwrap(),
            cas,
            clock: FakeClock::new(1_000),
            ids: SeqIdGen::new(),
        }
    }

    pub fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.dir.path().join("work").join(name);
        fs::write(&p, bytes).unwrap();
        p
    }

    pub fn ingest(&mut self, path: &Path) -> Result<IngestOutcome> {
        Ingestor::new(&mut self.store, &self.cas, &self.clock, &self.ids).ingest(path)
    }

    pub fn version_count(&self) -> i64 {
        self.store.conn().query_row("SELECT COUNT(*) FROM versions", [], |r| r.get(0)).unwrap()
    }

    pub fn item_count(&self) -> i64 {
        self.store.conn().query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0)).unwrap()
    }
}
