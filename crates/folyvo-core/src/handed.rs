//! Copies handed to another app through the share sheet. The share sheet only reports that the
//! app took the file (spike S12), so an event means "attached in Mail", never "sent".

use std::path::PathBuf;

use crate::cas::Cas;
use crate::error::{CoreError, Result};
use crate::export::{materialize, resolve};
use crate::model::{ItemId, ShareEvent, ShareEventKind, VersionId};
use crate::ports::Clock;
use crate::store::{repo, Store};

const KNOWN: [&str; 4] = ["mail", "airdrop", "messages", "copy"];
const OTHER: &str = "other:";
const MAX_OTHER_TITLE: usize = 58;

/// `mail`, `airdrop`, `messages`, `copy`, or `other:` followed by 1 to 58 printable characters.
pub fn validate_service(service: &str) -> Result<()> {
    let ok = KNOWN.contains(&service)
        || service.strip_prefix(OTHER).is_some_and(|title| {
            let n = title.chars().count();
            (1..=MAX_OTHER_TITLE).contains(&n) && !title.chars().any(char::is_control)
        });
    if ok {
        Ok(())
    } else {
        Err(CoreError::BadService(service.to_owned()))
    }
}

/// A read-only file with the item's name holding `version`'s bytes, for previews and the share sheet.
pub(crate) fn materialise(store: &Store, cas: &Cas, item: &ItemId, version: &VersionId) -> Result<PathBuf> {
    let (version, name) = resolve(store.conn(), item, version)?;
    materialize(cas, item, &version.content_hash, &name)
}

pub(crate) fn record_handed(
    store: &mut Store,
    clock: &dyn Clock,
    item: &ItemId,
    version: &VersionId,
    service: &str,
) -> Result<ShareEvent> {
    validate_service(service)?;
    let (version, _) = resolve(store.conn(), item, version)?;
    let event = ShareEvent {
        item_id: item.clone(),
        version_id: version.id,
        kind: ShareEventKind::Handed,
        service: Some(service.to_owned()),
        target_path: None,
        at: clock.now_unix_ms(),
    };
    repo::insert_event(store.conn(), &event, None)?;
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::share_events;
    use crate::testutil::Env;
    use std::fs;

    fn saved(e: &mut Env, name: &str, bytes: &[u8]) -> (ItemId, VersionId) {
        let p = e.file(name, bytes);
        let v = e.ingest(&p).unwrap().version().clone();
        (v.item_id, v.id)
    }

    #[test]
    fn materialising_twice_returns_the_same_read_only_path() {
        let mut e = Env::new();
        let (item, v) = saved(&mut e, "Teklif_Müşteri.docx", b"bytes");
        let a = materialise(&e.store, &e.cas, &item, &v).unwrap();
        let b = materialise(&e.store, &e.cas, &item, &v).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.file_name().unwrap().to_string_lossy(), "Teklif_Müşteri.docx");
        assert_eq!(fs::read(&a).unwrap(), b"bytes");
        assert!(fs::metadata(&a).unwrap().permissions().readonly());
    }

    #[test]
    fn a_handed_copy_is_recorded_with_its_service() {
        let mut e = Env::new();
        let (item, v) = saved(&mut e, "a.docx", b"one");
        let before = e.version_count();
        let ev = record_handed(&mut e.store, &e.clock, &item, &v, "mail").unwrap();
        assert_eq!(
            (ev.kind, ev.service.as_deref(), ev.target_path.as_deref()),
            (ShareEventKind::Handed, Some("mail"), None)
        );
        assert_eq!(share_events(&e.store, &item).unwrap(), vec![ev]);
        assert_eq!(e.version_count(), before, "handing a copy out never changes the versions");
    }

    #[test]
    fn services_are_validated() {
        for bad in ["", "Mail", "sent", "other:", "other:\u{7}", &format!("other:{}", "x".repeat(59))] {
            assert!(matches!(validate_service(bad), Err(CoreError::BadService(_))), "{bad:?} accepted");
        }
        for good in
            ["mail", "airdrop", "messages", "copy", "other:Blip", &format!("other:{}", "x".repeat(58))]
        {
            validate_service(good).unwrap_or_else(|e| panic!("{good:?} refused: {e}"));
        }
    }

    #[test]
    fn a_bad_service_writes_nothing() {
        let mut e = Env::new();
        let (item, v) = saved(&mut e, "a.docx", b"one");
        assert!(record_handed(&mut e.store, &e.clock, &item, &v, "sent").is_err());
        assert!(share_events(&e.store, &item).unwrap().is_empty());
    }

    #[test]
    fn a_version_of_another_item_is_refused_and_nothing_is_written() {
        let mut e = Env::new();
        let (a, _) = saved(&mut e, "a.docx", b"aaa");
        let (_, vb) = saved(&mut e, "b.docx", b"bbb");
        let err = record_handed(&mut e.store, &e.clock, &a, &vb, "mail").unwrap_err();
        assert!(matches!(err, CoreError::VersionMissing(_)), "{err}");
        assert!(share_events(&e.store, &a).unwrap().is_empty());
    }
}
