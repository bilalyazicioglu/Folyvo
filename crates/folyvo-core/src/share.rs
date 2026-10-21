use crate::error::{CoreError, Result};
use crate::model::{ItemId, ItemStatus, ShareEvent, ShareEventKind, VersionId};
use crate::ports::Clock;
use crate::store::{repo, Store};

pub fn mark_share_ready(
    store: &mut Store,
    clock: &dyn Clock,
    item: &ItemId,
    version: &VersionId,
) -> Result<()> {
    let now = clock.now_unix_ms();
    let tx = store.conn_mut().transaction()?;
    if !repo::item_exists(&tx, item)? {
        return Err(CoreError::ItemMissing);
    }
    match repo::version(&tx, version)? {
        Some(v) if &v.item_id == item => {}
        _ => return Err(CoreError::VersionMissing(version.clone())),
    }
    if repo::share_ready_version(&tx, item)?.as_ref() == Some(version) {
        return Ok(());
    }
    repo::set_share_ready(&tx, item, version, now)?;
    repo::insert_event(
        &tx,
        &ShareEvent {
            item_id: item.clone(),
            version_id: version.clone(),
            kind: ShareEventKind::Marked,
            target_path: None,
            at: now,
        },
        None,
    )?;
    tx.commit()?;
    Ok(())
}

pub fn item_status(store: &Store, item: &ItemId) -> Result<ItemStatus> {
    let c = store.conn();
    if !repo::item_exists(c, item)? {
        return Err(CoreError::ItemMissing);
    }
    let Some(ready_id) = repo::share_ready_version(c, item)? else {
        return Ok(ItemStatus::NotShared);
    };
    let ready = repo::version(c, &ready_id)?
        .ok_or_else(|| CoreError::Corrupt(format!("share-ready points at missing {ready_id}")))?;
    let latest = repo::latest_version(c, item)?
        .ok_or_else(|| CoreError::Corrupt(format!("{item} has a share-ready pointer but no versions")))?;
    Ok(if latest.content_hash == ready.content_hash {
        ItemStatus::Ready
    } else {
        ItemStatus::ChangedSinceReady
    })
}

pub fn share_events(store: &Store, item: &ItemId) -> Result<Vec<ShareEvent>> {
    repo::events_for_item(store.conn(), item)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ItemStatus, ShareEventKind};
    use crate::store::repo;
    use crate::testutil::Env;
    use std::fs;

    fn two_versions(e: &mut Env) -> (ItemId, VersionId, VersionId, std::path::PathBuf) {
        let p = e.file("a.docx", b"one");
        let v1 = e.ingest(&p).unwrap().version().clone();
        fs::write(&p, b"two").unwrap();
        let v2 = e.ingest(&p).unwrap().version().clone();
        (v1.item_id.clone(), v1.id, v2.id, p)
    }

    fn mark(e: &mut Env, item: &ItemId, v: &VersionId) -> Result<()> {
        mark_share_ready(&mut e.store, &e.clock, item, v)
    }

    #[test]
    fn nothing_is_shared_until_a_version_is_marked() {
        let mut e = Env::new();
        let (item, ..) = two_versions(&mut e);
        assert_eq!(item_status(&e.store, &item).unwrap(), ItemStatus::NotShared);
    }

    #[test]
    fn marking_the_latest_version_makes_the_item_ready() {
        let mut e = Env::new();
        let (item, _v1, v2, _) = two_versions(&mut e);
        mark(&mut e, &item, &v2).unwrap();
        assert_eq!(item_status(&e.store, &item).unwrap(), ItemStatus::Ready);
    }

    #[test]
    fn saving_after_marking_flips_the_status_to_changed() {
        let mut e = Env::new();
        let (item, v1, _v2, p) = two_versions(&mut e);
        mark(&mut e, &item, &v1).unwrap();
        assert_eq!(item_status(&e.store, &item).unwrap(), ItemStatus::ChangedSinceReady);
        fs::write(&p, b"three").unwrap();
        e.ingest(&p).unwrap();
        assert_eq!(item_status(&e.store, &item).unwrap(), ItemStatus::ChangedSinceReady);
    }

    #[test]
    fn status_compares_content_so_returning_to_marked_content_is_ready_again() {
        let mut e = Env::new();
        let (item, v1, _v2, p) = two_versions(&mut e);
        mark(&mut e, &item, &v1).unwrap();
        fs::write(&p, b"one").unwrap(); // back to the marked bytes
        e.ingest(&p).unwrap(); // becomes a new version with the same content as v1
        assert_eq!(item_status(&e.store, &item).unwrap(), ItemStatus::Ready);
    }

    #[test]
    fn re_marking_moves_the_pointer_and_keeps_the_old_mark_in_the_log() {
        let mut e = Env::new();
        let (item, v1, v2, _) = two_versions(&mut e);
        mark(&mut e, &item, &v1).unwrap();
        e.clock.advance(5);
        mark(&mut e, &item, &v2).unwrap();
        assert_eq!(repo::share_ready_version(e.store.conn(), &item).unwrap(), Some(v2.clone()));
        let log = share_events(&e.store, &item).unwrap();
        assert_eq!(log.len(), 2);
        assert!(log.iter().all(|ev| ev.kind == ShareEventKind::Marked));
        assert_eq!(log[0].version_id, v1);
        assert_eq!(log[1].version_id, v2);
        assert!(log[1].at > log[0].at);
    }

    #[test]
    fn marking_the_already_marked_version_records_nothing_new() {
        let mut e = Env::new();
        let (item, v1, ..) = two_versions(&mut e);
        mark(&mut e, &item, &v1).unwrap();
        mark(&mut e, &item, &v1).unwrap();
        assert_eq!(share_events(&e.store, &item).unwrap().len(), 1);
    }

    #[test]
    fn marking_a_version_of_another_item_is_refused_and_changes_nothing() {
        let mut e = Env::new();
        let (item_a, v1, ..) = two_versions(&mut e);
        let pb = e.file("b.docx", b"other");
        let vb = e.ingest(&pb).unwrap().version().clone();
        mark(&mut e, &item_a, &v1).unwrap();
        let err = mark(&mut e, &item_a, &vb.id).unwrap_err();
        assert!(matches!(err, CoreError::VersionMissing(_)), "{err}");
        assert_eq!(repo::share_ready_version(e.store.conn(), &item_a).unwrap(), Some(v1));
    }

    #[test]
    fn unknown_item_is_reported_as_missing() {
        let mut e = Env::new();
        let ghost = ItemId::from_suffix("dead");
        assert!(matches!(item_status(&e.store, &ghost), Err(CoreError::ItemMissing)));
        let v = VersionId::from_suffix("beef");
        assert!(matches!(mark(&mut e, &ghost, &v), Err(CoreError::ItemMissing)));
    }
}
