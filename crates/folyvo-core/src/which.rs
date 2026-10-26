//! "Which one is this?": recognise a file by its exact content. Never a guess: a file that was
//! opened and saved again elsewhere matches nothing.

use std::collections::HashMap;
use std::path::Path;

use crate::engine::{version_views, VersionView};
use crate::error::{CoreError, Result};
use crate::export::file_hash;
use crate::model::{ContentHash, ItemId, ShareEvent};
use crate::store::{repo, Store};

/// One saved version whose content is exactly the file's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub item_id: ItemId,
    pub name: String,
    pub path: Option<String>,
    pub version: VersionView,
    /// Copies of this version that were saved or handed out, oldest first.
    pub events: Vec<ShareEvent>,
}

/// Every version holding exactly `hash`, across items, earliest first.
pub(crate) fn matches_for_hash(store: &Store, hash: &ContentHash) -> Result<Vec<Match>> {
    let c = store.conn();
    let mut views: HashMap<ItemId, Vec<VersionView>> = HashMap::new();
    let mut out = Vec::new();
    for v in repo::versions_by_hash(c, hash)? {
        if !views.contains_key(&v.item_id) {
            views.insert(v.item_id.clone(), version_views(c, &v.item_id)?);
        }
        let version = views[&v.item_id]
            .iter()
            .find(|view| view.id == v.id)
            .cloned()
            .ok_or_else(|| CoreError::Corrupt(format!("{} vanished while reading", v.id)))?;
        let (name, _) = repo::item_flags(c, &v.item_id)?.ok_or(CoreError::ItemMissing)?;
        let events =
            repo::events_for_item(c, &v.item_id)?.into_iter().filter(|e| e.version_id == v.id).collect();
        out.push(Match {
            path: repo::current_path(c, &v.item_id)?,
            item_id: v.item_id,
            name,
            version,
            events,
        });
    }
    Ok(out)
}

/// Hash the file at `path` and look it up. Directories are `NotAFile`; a missing path is an I/O error.
#[allow(dead_code)] // used by the Vault (CLI) in the next task
pub(crate) fn which_one(store: &Store, path: &Path) -> Result<Vec<Match>> {
    let hash = hash_of_file(path)?;
    matches_for_hash(store, &hash)
}

/// The content hash of a regular file; `NotAFile` for anything else.
pub(crate) fn hash_of_file(path: &Path) -> Result<ContentHash> {
    let meta = std::fs::metadata(path).map_err(|e| CoreError::io(path, e))?;
    if !meta.is_file() {
        return Err(CoreError::NotAFile(path.to_path_buf()));
    }
    file_hash(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::Exporter;
    use crate::handed::record_handed;
    use crate::model::{ShareEventKind, VersionId};
    use crate::restore::Restorer;
    use crate::testutil::Env;
    use std::fs;

    fn save(e: &mut Env, name: &str, bytes: &[u8]) -> (ItemId, VersionId, std::path::PathBuf) {
        let p = e.dir.path().join(name);
        fs::write(&p, bytes).unwrap();
        let v = e.ingest(&p).unwrap().version().clone();
        (v.item_id, v.id, p)
    }

    fn outside(e: &Env, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let d = e.dir.path().join("elsewhere");
        fs::create_dir_all(&d).unwrap();
        let p = d.join(name);
        fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn a_copy_of_an_older_version_is_recognised() {
        let mut e = Env::new();
        let (item, v1, p) = save(&mut e, "Teklif.docx", b"one");
        fs::write(&p, b"two").unwrap();
        e.ingest(&p).unwrap();
        let copy = outside(&e, "Teklif.docx", b"one");
        let m = which_one(&e.store, &copy).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].item_id.clone(), m[0].version.id.clone()), (item, v1));
        assert_eq!(m[0].name, "Teklif.docx");
        assert!(!m[0].version.code.is_empty());
    }

    #[test]
    fn a_file_edited_after_it_left_matches_nothing() {
        let mut e = Env::new();
        save(&mut e, "a.docx", b"one");
        let copy = outside(&e, "a.docx", b"one!");
        assert!(which_one(&e.store, &copy).unwrap().is_empty());
    }

    #[test]
    fn identical_bytes_in_two_items_return_both_earliest_first() {
        let mut e = Env::new();
        let (a, _, _) = save(&mut e, "a.docx", b"same");
        e.clock.advance(10);
        let (b, _, _) = save(&mut e, "b.docx", b"same");
        let copy = outside(&e, "x.docx", b"same");
        let items: Vec<ItemId> = which_one(&e.store, &copy).unwrap().into_iter().map(|m| m.item_id).collect();
        assert_eq!(items, vec![a, b]);
    }

    #[test]
    fn a_restored_version_and_its_source_both_match() {
        let mut e = Env::new();
        let (item, v1, p) = save(&mut e, "a.docx", b"one");
        fs::write(&p, b"two").unwrap();
        e.ingest(&p).unwrap();
        let restored = Restorer::new(&mut e.store, &e.cas, &e.clock, &e.ids).restore(&item, &v1).unwrap();
        let copy = outside(&e, "a.docx", b"one");
        let ids: Vec<VersionId> =
            which_one(&e.store, &copy).unwrap().into_iter().map(|m| m.version.id).collect();
        assert_eq!(ids, vec![v1, restored.id]);
    }

    #[test]
    fn copies_of_the_version_come_with_the_match() {
        let mut e = Env::new();
        let (item, v1, _) = save(&mut e, "a.docx", b"one");
        record_handed(&mut e.store, &e.clock, &item, &v1, "mail").unwrap();
        let out = e.dir.path().join("out");
        fs::create_dir_all(&out).unwrap();
        Exporter::new(&mut e.store, &e.cas, &e.clock).export(&item, &v1, &out).unwrap();
        let copy = outside(&e, "a.docx", b"one");
        let kinds: Vec<ShareEventKind> =
            which_one(&e.store, &copy).unwrap()[0].events.iter().map(|ev| ev.kind).collect();
        assert_eq!(kinds, vec![ShareEventKind::Handed, ShareEventKind::Exported]);
    }

    #[test]
    fn a_directory_is_not_a_file_and_a_missing_path_is_an_io_error() {
        let e = Env::new();
        assert!(matches!(which_one(&e.store, e.dir.path()), Err(CoreError::NotAFile(_))));
        assert!(matches!(which_one(&e.store, &e.dir.path().join("nope.docx")), Err(CoreError::Io { .. })));
    }
}
