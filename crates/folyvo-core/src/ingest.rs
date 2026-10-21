use std::path::Path;

use crate::canonical::canonical_key;
use crate::cas::Cas;
use crate::error::{CoreError, Result};
use crate::model::{ItemId, Version, VersionId, VersionOrigin};
use crate::ports::{Clock, IdGen};
use crate::store::{repo, Store};

pub const STABLE_READ_ATTEMPTS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestOutcome {
    Created(Version),
    Unchanged(Version),
}

impl IngestOutcome {
    pub fn version(&self) -> &Version {
        match self {
            IngestOutcome::Created(v) | IngestOutcome::Unchanged(v) => v,
        }
    }
}

pub struct Ingestor<'a> {
    store: &'a mut Store,
    cas: &'a Cas,
    clock: &'a dyn Clock,
    ids: &'a dyn IdGen,
}

impl<'a> Ingestor<'a> {
    pub fn new(store: &'a mut Store, cas: &'a Cas, clock: &'a dyn Clock, ids: &'a dyn IdGen) -> Self {
        Self { store, cas, clock, ids }
    }

    pub fn ingest(&mut self, path: &Path) -> Result<IngestOutcome> {
        let key = canonical_key(path)?;
        let stored = self.store_object(path)?;
        let now = self.clock.now_unix_ms();

        let tx = self.store.conn_mut().transaction()?;
        let (item_id, previous) = match repo::item_at_key(&tx, &key)? {
            Some(id) => {
                let prev = repo::latest_version(&tx, &id)?;
                (id, prev)
            }
            None => {
                let id = ItemId::from_suffix(&self.ids.next_suffix());
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                repo::insert_item(&tx, &id, &name, now)?;
                repo::set_current_location(&tx, &id, &path.to_string_lossy(), &key, now)?;
                (id, None)
            }
        };

        if let Some(prev) = &previous {
            if prev.content_hash == stored.hash {
                repo::set_missing(&tx, &item_id, false)?;
                tx.commit()?;
                return Ok(IngestOutcome::Unchanged(prev.clone()));
            }
        }

        repo::upsert_object(&tx, &stored.hash, stored.size, now)?;
        let version = Version {
            id: VersionId::from_suffix(&self.ids.next_suffix()),
            item_id: item_id.clone(),
            seq: previous.as_ref().map(|p| p.seq + 1).unwrap_or(1),
            content_hash: stored.hash,
            size: stored.size,
            created_at: now,
            origin: if previous.is_some() { VersionOrigin::Auto } else { VersionOrigin::Initial },
            restored_from: None,
            note: None,
        };
        repo::insert_version(&tx, &version)?;
        repo::set_missing(&tx, &item_id, false)?;
        tx.commit()?;
        Ok(IngestOutcome::Created(version))
    }

    fn store_object(&self, path: &Path) -> Result<crate::cas::Stored> {
        let mut attempt = 1;
        loop {
            match self.cas.put_file(path) {
                Err(CoreError::Unstable(_)) if attempt < STABLE_READ_ATTEMPTS => attempt += 1,
                other => return other,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::VersionOrigin;
    use crate::store::repo;
    use crate::testutil::Env;
    use std::fs;

    fn env() -> Env {
        Env::new()
    }

    #[test]
    fn first_ingest_creates_an_item_and_an_initial_version() {
        let mut e = env();
        let p = e.file("a.docx", b"one");
        let out = e.ingest(&p).unwrap();
        let IngestOutcome::Created(v) = out else { panic!("expected Created") };
        assert_eq!(v.seq, 1);
        assert_eq!(v.origin, VersionOrigin::Initial);
        assert_eq!(v.content_hash, crate::model::ContentHash::of_bytes(b"one"));
        assert_eq!(v.created_at, 1_000);
        assert_eq!(e.item_count(), 1);
        assert!(e.cas.contains(&v.content_hash));
    }

    #[test]
    fn saving_identical_bytes_again_does_nothing() {
        let mut e = env();
        let p = e.file("a.docx", b"one");
        e.ingest(&p).unwrap();
        let out = e.ingest(&p).unwrap();
        assert!(matches!(out, IngestOutcome::Unchanged(_)));
        assert_eq!(e.version_count(), 1);
    }

    #[test]
    fn changed_bytes_create_the_next_version_for_the_same_item() {
        let mut e = env();
        let p = e.file("a.docx", b"one");
        let first = e.ingest(&p).unwrap().version().clone();
        e.clock.advance(60_000);
        fs::write(&p, b"two").unwrap();
        let second = e.ingest(&p).unwrap().version().clone();
        assert_eq!(second.seq, 2);
        assert_eq!(second.origin, VersionOrigin::Auto);
        assert_eq!(second.item_id, first.item_id);
        assert_eq!(second.created_at, 61_000);
        assert_eq!(e.item_count(), 1);
    }

    #[test]
    fn going_back_to_older_content_is_a_new_version_not_a_no_op() {
        let mut e = env();
        let p = e.file("a.docx", b"one");
        e.ingest(&p).unwrap();
        fs::write(&p, b"two").unwrap();
        e.ingest(&p).unwrap();
        fs::write(&p, b"one").unwrap();
        let third = e.ingest(&p).unwrap();
        assert!(matches!(third, IngestOutcome::Created(ref v) if v.seq == 3));
        assert_eq!(e.version_count(), 3);
    }

    // Review Focus 1: an empty file is a valid file and gets a version.
    #[test]
    fn an_empty_file_gets_a_version() {
        let mut e = env();
        let p = e.file("blank.txt", b"");
        let v = e.ingest(&p).unwrap().version().clone();
        assert_eq!(v.size, 0);
        assert_eq!(v.seq, 1);
    }

    #[test]
    fn a_file_reappearing_after_deletion_clears_the_missing_flag() {
        let mut e = env();
        let p = e.file("a.docx", b"one");
        let item = e.ingest(&p).unwrap().version().item_id.clone();
        repo::set_missing(e.store.conn(), &item, true).unwrap();
        fs::write(&p, b"two").unwrap();
        e.ingest(&p).unwrap();
        let m: i64 = e.store.conn().query_row("SELECT missing FROM items", [], |r| r.get(0)).unwrap();
        assert_eq!(m, 0);
    }

    #[test]
    fn a_missing_file_is_an_error_and_creates_nothing() {
        let mut e = env();
        let p = e.dir.path().join("work").join("ghost.docx");
        assert!(e.ingest(&p).is_err());
        assert_eq!(e.item_count(), 0);
    }

    #[test]
    fn a_directory_is_refused_and_creates_nothing() {
        let mut e = env();
        let d = e.dir.path().join("work").join("Deck.key");
        fs::create_dir(&d).unwrap();
        assert!(matches!(e.ingest(&d), Err(CoreError::NotAFile(_))));
        assert_eq!(e.item_count(), 0);
    }

    #[test]
    fn two_different_files_become_two_items() {
        let mut e = env();
        let (a, b) = (e.file("a.docx", b"x"), e.file("b.docx", b"x"));
        e.ingest(&a).unwrap();
        e.ingest(&b).unwrap();
        assert_eq!(e.item_count(), 2);
        assert_eq!(e.version_count(), 2);
    }
}
