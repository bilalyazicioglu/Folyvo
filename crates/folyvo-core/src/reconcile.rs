use std::path::Path;

use crate::canonical::canonical_key;
use crate::error::{CoreError, Result};
use crate::model::{ContentHash, ItemId};
use crate::ports::Clock;
use crate::store::{repo, Store};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub inode: Option<u64>,
    pub size: u64,
    pub mtime_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    pub item_id: ItemId,
    pub inode: Option<u64>,
    pub size: u64,
    pub mtime_ms: i64,
    pub hash: ContentHash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Match {
    Same(ItemId),
    NewItem,
    Ambiguous,
}

pub fn match_candidate(
    candidate: &Candidate,
    vanished: &[Known],
    hash_of_candidate: &mut dyn FnMut() -> Result<ContentHash>,
) -> Result<Match> {
    if vanished.is_empty() {
        return Ok(Match::NewItem);
    }
    if let Some(inode) = candidate.inode {
        let by_inode: Vec<&Known> = vanished.iter().filter(|k| k.inode == Some(inode)).collect();
        match by_inode.len() {
            0 => {}
            1 => return Ok(Match::Same(by_inode[0].item_id.clone())),
            _ => return Ok(Match::Ambiguous),
        }
    }
    if candidate.size == 0 {
        // Empty content is no evidence of identity: any two empty files share a hash.
        return Ok(Match::NewItem);
    }
    let hash = hash_of_candidate()?;
    let by_hash: Vec<&Known> = vanished.iter().filter(|k| k.hash == hash).collect();
    match by_hash.len() {
        0 => Ok(Match::NewItem),
        1 => Ok(Match::Same(by_hash[0].item_id.clone())),
        _ => {
            let by_mtime: Vec<&&Known> =
                by_hash.iter().filter(|k| k.mtime_ms == candidate.mtime_ms).collect();
            Ok(if by_mtime.len() == 1 { Match::Same(by_mtime[0].item_id.clone()) } else { Match::Ambiguous })
        }
    }
}

/// Point an existing item at its new path. History and identity are untouched.
pub fn relocate(store: &mut Store, clock: &dyn Clock, item: &ItemId, new_path: &Path) -> Result<()> {
    let key = canonical_key(new_path)?;
    let now = clock.now_unix_ms();
    let tx = store.conn_mut().transaction()?;
    if !repo::item_exists(&tx, item)? {
        return Err(CoreError::ItemMissing);
    }
    if let Some(owner) = repo::item_at_key(&tx, &key)? {
        if &owner != item {
            return Err(CoreError::PathTaken(new_path.to_path_buf()));
        }
    }
    repo::set_current_location(&tx, item, &new_path.to_string_lossy(), &key, now)?;
    repo::set_missing(&tx, item, false)?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ContentHash;
    use crate::store::repo;
    use crate::testutil::Env;
    use std::cell::Cell;
    use std::fs;

    fn known(item: &str, inode: Option<u64>, bytes: &[u8], mtime_ms: i64) -> Known {
        Known {
            item_id: ItemId::from_suffix(item),
            inode,
            size: bytes.len() as u64,
            mtime_ms,
            hash: ContentHash::of_bytes(bytes),
        }
    }

    fn cand(inode: Option<u64>, bytes: &[u8], mtime_ms: i64) -> Candidate {
        Candidate { inode, size: bytes.len() as u64, mtime_ms }
    }

    fn run(c: &Candidate, vanished: &[Known], bytes: &[u8]) -> (Match, usize) {
        let calls = Cell::new(0usize);
        let mut hash = || {
            calls.set(calls.get() + 1);
            Ok(ContentHash::of_bytes(bytes))
        };
        let m = match_candidate(c, vanished, &mut hash).unwrap();
        (m, calls.get())
    }

    #[test]
    fn nothing_vanished_means_a_new_item_and_no_hashing() {
        let (m, hashed) = run(&cand(Some(1), b"x", 5), &[], b"x");
        assert_eq!(m, Match::NewItem);
        assert_eq!(hashed, 0);
    }

    #[test]
    fn a_unique_inode_match_wins_even_if_the_content_was_edited_and_skips_hashing() {
        let v = [known("a1", Some(7), b"old", 1), known("b2", Some(8), b"zzz", 1)];
        let (m, hashed) = run(&cand(Some(7), b"edited", 9), &v, b"edited");
        assert_eq!(m, Match::Same(ItemId::from_suffix("a1")));
        assert_eq!(hashed, 0);
    }

    #[test]
    fn without_inode_information_a_unique_hash_match_is_the_same_item() {
        let v = [known("a1", None, b"same", 1), known("b2", None, b"other", 1)];
        let (m, hashed) = run(&cand(None, b"same", 2), &v, b"same");
        assert_eq!(m, Match::Same(ItemId::from_suffix("a1")));
        assert_eq!(hashed, 1);
    }

    #[test]
    fn an_empty_file_is_never_matched_by_hash() {
        let v = [known("a1", None, b"", 1)];
        let (m, hashed) = run(&cand(None, b"", 1), &v, b"");
        assert_eq!(m, Match::NewItem);
        assert_eq!(hashed, 0);
    }

    #[test]
    fn two_vanished_items_with_the_same_hash_are_ambiguous() {
        let v = [known("a1", None, b"same", 1), known("b2", None, b"same", 2)];
        let (m, _) = run(&cand(None, b"same", 99), &v, b"same");
        assert_eq!(m, Match::Ambiguous);
    }

    #[test]
    fn mtime_breaks_a_hash_tie_when_it_singles_out_one() {
        let v = [known("a1", None, b"same", 1), known("b2", None, b"same", 2)];
        let (m, _) = run(&cand(None, b"same", 2), &v, b"same");
        assert_eq!(m, Match::Same(ItemId::from_suffix("b2")));
    }

    #[test]
    fn an_inode_that_matches_nobody_falls_back_to_the_hash() {
        let v = [known("a1", Some(7), b"same", 1)];
        let (m, hashed) = run(&cand(Some(99), b"same", 1), &v, b"same");
        assert_eq!(m, Match::Same(ItemId::from_suffix("a1")));
        assert_eq!(hashed, 1);
    }

    #[test]
    fn no_match_at_all_is_a_new_item() {
        let v = [known("a1", None, b"one", 1)];
        let (m, _) = run(&cand(None, b"two", 1), &v, b"two");
        assert_eq!(m, Match::NewItem);
    }

    #[test]
    fn a_hashing_error_is_passed_up() {
        let v = [known("a1", None, b"x", 1)];
        let mut failing = || Err(CoreError::Corrupt("boom".into()));
        assert!(match_candidate(&cand(None, b"x", 1), &v, &mut failing).is_err());
    }

    #[test]
    fn relocating_keeps_the_item_and_the_history_and_ingest_sees_no_change() {
        let mut e = Env::new();
        let old = e.file("draft.docx", b"one");
        let v = e.ingest(&old).unwrap().version().clone();
        let new = old.with_file_name("final.docx");
        fs::rename(&old, &new).unwrap();
        relocate(&mut e.store, &e.clock, &v.item_id, &new).unwrap();

        let key = crate::canonical::canonical_key(&new).unwrap();
        assert_eq!(repo::item_at_key(e.store.conn(), &key).unwrap(), Some(v.item_id.clone()));
        let out = e.ingest(&new).unwrap();
        assert!(matches!(out, crate::ingest::IngestOutcome::Unchanged(_)));
        assert_eq!(e.item_count(), 1);
        assert_eq!(e.version_count(), 1);
    }

    #[test]
    fn a_case_only_rename_does_not_create_a_second_item() {
        let mut e = Env::new();
        let old = e.file("a.docx", b"one");
        let v = e.ingest(&old).unwrap().version().clone();
        let new = old.with_file_name("A.docx");
        fs::rename(&old, &new).unwrap();
        relocate(&mut e.store, &e.clock, &v.item_id, &new).unwrap();
        e.ingest(&new).unwrap();
        assert_eq!(e.item_count(), 1);
        let stored = repo::current_path(e.store.conn(), &v.item_id).unwrap().unwrap();
        assert!(stored.ends_with("A.docx"), "the new spelling is remembered, got {stored}");
    }

    #[test]
    fn relocating_onto_a_path_owned_by_another_item_is_refused_and_changes_nothing() {
        let mut e = Env::new();
        let pa = e.file("a.docx", b"a");
        let pb = e.file("b.docx", b"b");
        let va = e.ingest(&pa).unwrap().version().clone();
        e.ingest(&pb).unwrap();
        let err = relocate(&mut e.store, &e.clock, &va.item_id, &pb).unwrap_err();
        assert!(matches!(err, CoreError::PathTaken(_)), "{err}");
        let key = crate::canonical::canonical_key(&pa).unwrap();
        assert_eq!(repo::item_at_key(e.store.conn(), &key).unwrap(), Some(va.item_id));
    }

    #[test]
    fn relocating_an_unknown_item_is_reported() {
        let mut e = Env::new();
        let p = e.file("a.docx", b"a");
        let err = relocate(&mut e.store, &e.clock, &ItemId::from_suffix("dead"), &p).unwrap_err();
        assert!(matches!(err, CoreError::ItemMissing), "{err}");
    }
}
