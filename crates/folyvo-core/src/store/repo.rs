use rusqlite::types::Type;
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::{CoreError, Result};
use crate::model::{ContentHash, ItemId, ShareEvent, ShareEventKind, Version, VersionId, VersionOrigin};

const VERSION_COLS: &str = "id, item_id, seq, content_hash, size, created_at, origin, restored_from, note";

pub(crate) fn insert_item(c: &Connection, id: &ItemId, name: &str, now: i64) -> Result<()> {
    c.execute(
        "INSERT INTO items(id, display_name, created_at) VALUES (?1, ?2, ?3)",
        params![id.as_str(), name, now],
    )?;
    Ok(())
}

pub(crate) fn set_missing(c: &Connection, id: &ItemId, missing: bool) -> Result<()> {
    c.execute("UPDATE items SET missing = ?2 WHERE id = ?1", params![id.as_str(), missing as i64])?;
    Ok(())
}

pub(crate) fn set_current_location(
    c: &Connection,
    item: &ItemId,
    path: &str,
    key: &str,
    now: i64,
) -> Result<()> {
    c.execute("UPDATE locations SET is_current = 0 WHERE item_id = ?1 AND is_current = 1", [item.as_str()])?;
    c.execute(
        "INSERT INTO locations(item_id, path, canonical_key, observed_at, is_current) \
         VALUES (?1, ?2, ?3, ?4, 1)",
        params![item.as_str(), path, key, now],
    )?;
    Ok(())
}

pub(crate) fn item_at_key(c: &Connection, key: &str) -> Result<Option<ItemId>> {
    let id: Option<String> = c
        .query_row("SELECT item_id FROM locations WHERE canonical_key = ?1 AND is_current = 1", [key], |r| {
            r.get(0)
        })
        .optional()?;
    id.map(|s| ItemId::parse(&s).map_err(|e| CoreError::Corrupt(e.to_string()))).transpose()
}

pub(crate) fn current_path(c: &Connection, item: &ItemId) -> Result<Option<String>> {
    Ok(c.query_row(
        "SELECT path FROM locations WHERE item_id = ?1 AND is_current = 1",
        [item.as_str()],
        |r| r.get(0),
    )
    .optional()?)
}

pub(crate) fn upsert_object(c: &Connection, hash: &ContentHash, size: u64, now: i64) -> Result<()> {
    c.execute(
        "INSERT OR IGNORE INTO objects(content_hash, size, stored_at) VALUES (?1, ?2, ?3)",
        params![hash.as_str(), size as i64, now],
    )?;
    Ok(())
}

pub(crate) fn insert_version(c: &Connection, v: &Version) -> Result<()> {
    c.execute(
        "INSERT INTO versions(id, item_id, seq, content_hash, size, created_at, origin, restored_from, note) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            v.id.as_str(),
            v.item_id.as_str(),
            v.seq,
            v.content_hash.as_str(),
            v.size as i64,
            v.created_at,
            v.origin.as_str(),
            v.restored_from.as_ref().map(|r| r.as_str()),
            v.note,
        ],
    )?;
    Ok(())
}

fn conv(idx: usize, e: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, e.into())
}

fn row_to_version(r: &Row<'_>) -> rusqlite::Result<Version> {
    let id: String = r.get(0)?;
    let item_id: String = r.get(1)?;
    let hash: String = r.get(3)?;
    let origin: String = r.get(6)?;
    let restored_from: Option<String> = r.get(7)?;
    Ok(Version {
        id: VersionId::parse(&id).map_err(|e| conv(0, e))?,
        item_id: ItemId::parse(&item_id).map_err(|e| conv(1, e))?,
        seq: r.get(2)?,
        content_hash: ContentHash::parse(&hash).map_err(|e| conv(3, e))?,
        size: r.get::<_, i64>(4)? as u64,
        created_at: r.get(5)?,
        origin: VersionOrigin::parse(&origin).ok_or_else(|| conv(6, "unknown origin"))?,
        restored_from: restored_from.map(|s| VersionId::parse(&s).map_err(|e| conv(7, e))).transpose()?,
        note: r.get(8)?,
    })
}

pub(crate) fn latest_version(c: &Connection, item: &ItemId) -> Result<Option<Version>> {
    Ok(c.query_row(
        &format!("SELECT {VERSION_COLS} FROM versions WHERE item_id = ?1 ORDER BY seq DESC LIMIT 1"),
        [item.as_str()],
        row_to_version,
    )
    .optional()?)
}

pub(crate) fn version(c: &Connection, id: &VersionId) -> Result<Option<Version>> {
    Ok(c.query_row(
        &format!("SELECT {VERSION_COLS} FROM versions WHERE id = ?1"),
        [id.as_str()],
        row_to_version,
    )
    .optional()?)
}

pub(crate) fn versions_newest_first(c: &Connection, item: &ItemId) -> Result<Vec<Version>> {
    let mut stmt =
        c.prepare(&format!("SELECT {VERSION_COLS} FROM versions WHERE item_id = ?1 ORDER BY seq DESC"))?;
    let rows = stmt.query_map([item.as_str()], row_to_version)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Every version holding exactly `hash`, across items, earliest first.
pub(crate) fn versions_by_hash(c: &Connection, hash: &ContentHash) -> Result<Vec<Version>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {VERSION_COLS} FROM versions WHERE content_hash = ?1 ORDER BY created_at ASC, item_id ASC, seq ASC"
    ))?;
    let rows = stmt.query_map([hash.as_str()], row_to_version)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn item_exists(c: &Connection, item: &ItemId) -> Result<bool> {
    Ok(c.query_row("SELECT 1 FROM items WHERE id = ?1", [item.as_str()], |_| Ok(())).optional()?.is_some())
}

pub(crate) fn display_name(c: &Connection, item: &ItemId) -> Result<Option<String>> {
    Ok(c.query_row("SELECT display_name FROM items WHERE id = ?1", [item.as_str()], |r| r.get(0))
        .optional()?)
}

pub(crate) fn insert_event(c: &Connection, e: &ShareEvent, target_key: Option<&str>) -> Result<()> {
    c.execute(
        "INSERT INTO share_events(item_id, version_id, kind, service, target_path, target_key, at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            e.item_id.as_str(),
            e.version_id.as_str(),
            e.kind.as_str(),
            e.service,
            e.target_path,
            target_key,
            e.at
        ],
    )?;
    Ok(())
}

pub(crate) fn events_for_item(c: &Connection, item: &ItemId) -> Result<Vec<ShareEvent>> {
    let mut stmt = c.prepare(
        "SELECT item_id, version_id, kind, service, target_path, at FROM share_events WHERE item_id = ?1 ORDER BY id ASC",
    )?;
    let rows = stmt.query_map([item.as_str()], |r| {
        let item_id: String = r.get(0)?;
        let version_id: String = r.get(1)?;
        let kind: String = r.get(2)?;
        Ok(ShareEvent {
            item_id: ItemId::parse(&item_id).map_err(|e| conv(0, e))?,
            version_id: VersionId::parse(&version_id).map_err(|e| conv(1, e))?,
            kind: ShareEventKind::parse(&kind).ok_or_else(|| conv(2, "unknown event kind"))?,
            service: r.get(3)?,
            target_path: r.get(4)?,
            at: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[allow(dead_code)] // used by the watcher (Task 15)
pub(crate) fn is_export_target(c: &Connection, key: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT 1 FROM share_events WHERE kind = 'exported' AND target_key = ?1 LIMIT 1",
        [key],
        |_| Ok(()),
    )
    .optional()?
    .is_some())
}

pub(crate) fn insert_root(c: &Connection, path: &str, now: i64) -> Result<()> {
    c.execute("INSERT OR IGNORE INTO watched_roots(path, added_at) VALUES (?1, ?2)", params![path, now])?;
    Ok(())
}

pub(crate) fn delete_root(c: &Connection, path: &str) -> Result<()> {
    c.execute("DELETE FROM watched_roots WHERE path = ?1", [path])?;
    Ok(())
}

pub(crate) fn list_roots(c: &Connection) -> Result<Vec<String>> {
    let mut stmt = c.prepare("SELECT path FROM watched_roots ORDER BY id")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<String>>>()?)
}

pub(crate) fn all_current(c: &Connection) -> Result<Vec<(ItemId, String, String)>> {
    let mut stmt = c.prepare(
        "SELECT item_id, canonical_key, path FROM locations WHERE is_current = 1 ORDER BY item_id",
    )?;
    let rows = stmt.query_map([], |r| {
        let id: String = r.get(0)?;
        Ok((ItemId::parse(&id).map_err(|e| conv(0, e))?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn current_key(c: &Connection, item: &ItemId) -> Result<Option<String>> {
    Ok(c.query_row(
        "SELECT canonical_key FROM locations WHERE item_id = ?1 AND is_current = 1",
        [item.as_str()],
        |r| r.get(0),
    )
    .optional()?)
}

pub(crate) fn item_at_path(c: &Connection, path: &str) -> Result<Option<ItemId>> {
    let id: Option<String> = c
        .query_row("SELECT item_id FROM locations WHERE path = ?1 AND is_current = 1", [path], |r| r.get(0))
        .optional()?;
    id.map(|s| ItemId::parse(&s).map_err(|e| CoreError::Corrupt(e.to_string()))).transpose()
}

pub(crate) fn all_object_hashes(c: &Connection) -> Result<Vec<String>> {
    let mut stmt = c.prepare("SELECT content_hash FROM objects")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<String>>>()?)
}

pub(crate) fn item_flags(c: &Connection, item: &ItemId) -> Result<Option<(String, bool)>> {
    Ok(c.query_row("SELECT display_name, missing FROM items WHERE id = ?1", [item.as_str()], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? != 0))
    })
    .optional()?)
}

pub(crate) fn set_note(c: &Connection, version: &VersionId, note: Option<&str>) -> Result<bool> {
    Ok(c.execute("UPDATE versions SET note = ?2 WHERE id = ?1", params![version.as_str(), note])? > 0)
}

pub(crate) fn recent_items(c: &Connection, limit: u32) -> Result<Vec<ItemId>> {
    let mut stmt = c.prepare(
        "SELECT item_id FROM versions GROUP BY item_id ORDER BY MAX(created_at) DESC, MAX(seq) DESC, item_id ASC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |r| {
        let id: String = r.get(0)?;
        ItemId::parse(&id).map_err(|e| conv(0, e))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn counts(c: &Connection) -> Result<(u64, u64, u64)> {
    let (i, v, b): (i64, i64, i64) = c.query_row(
        "SELECT (SELECT COUNT(*) FROM items), (SELECT COUNT(*) FROM versions), (SELECT COALESCE(SUM(size), 0) FROM objects)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    Ok((i as u64, v as u64, b as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    fn item(n: &str) -> ItemId {
        ItemId::from_suffix(n)
    }

    fn ver(n: &str, item_id: &ItemId, seq: i64, hash: &ContentHash) -> Version {
        Version {
            id: VersionId::from_suffix(n),
            item_id: item_id.clone(),
            seq,
            content_hash: hash.clone(),
            size: 3,
            created_at: 100 + seq,
            origin: if seq == 1 { VersionOrigin::Initial } else { VersionOrigin::Auto },
            restored_from: None,
            note: None,
        }
    }

    #[test]
    fn current_location_moves_and_old_key_stops_resolving() {
        let s = Store::open_in_memory().unwrap();
        let c = s.conn();
        let a = item("a1");
        insert_item(c, &a, "a.docx", 1).unwrap();
        set_current_location(c, &a, "/w/a.docx", "k-old", 1).unwrap();
        assert_eq!(item_at_key(c, "k-old").unwrap(), Some(a.clone()));
        set_current_location(c, &a, "/w/b.docx", "k-new", 2).unwrap();
        assert_eq!(item_at_key(c, "k-old").unwrap(), None);
        assert_eq!(item_at_key(c, "k-new").unwrap(), Some(a.clone()));
        assert_eq!(current_path(c, &a).unwrap().as_deref(), Some("/w/b.docx"));
    }

    #[test]
    fn versions_round_trip_and_latest_is_highest_seq() {
        let s = Store::open_in_memory().unwrap();
        let c = s.conn();
        let a = item("a1");
        insert_item(c, &a, "a.docx", 1).unwrap();
        let (h1, h2) = (ContentHash::of_bytes(b"one"), ContentHash::of_bytes(b"two"));
        upsert_object(c, &h1, 3, 1).unwrap();
        upsert_object(c, &h2, 3, 1).unwrap();
        let (v1, v2) = (ver("01", &a, 1, &h1), ver("02", &a, 2, &h2));
        insert_version(c, &v1).unwrap();
        insert_version(c, &v2).unwrap();
        assert_eq!(latest_version(c, &a).unwrap(), Some(v2.clone()));
        assert_eq!(version(c, &v1.id).unwrap(), Some(v1.clone()));
        assert_eq!(versions_newest_first(c, &a).unwrap(), vec![v2, v1]);
    }

    #[test]
    fn a_version_cannot_be_inserted_before_its_object_exists() {
        let s = Store::open_in_memory().unwrap();
        let c = s.conn();
        let a = item("a1");
        insert_item(c, &a, "a.docx", 1).unwrap();
        let v = ver("01", &a, 1, &ContentHash::of_bytes(b"never stored"));
        assert!(insert_version(c, &v).is_err());
        assert_eq!(latest_version(c, &a).unwrap(), None);
    }

    #[test]
    fn upserting_the_same_object_twice_is_fine() {
        let s = Store::open_in_memory().unwrap();
        let h = ContentHash::of_bytes(b"x");
        upsert_object(s.conn(), &h, 1, 1).unwrap();
        upsert_object(s.conn(), &h, 1, 2).unwrap();
    }

    #[test]
    fn missing_flag_can_be_set_and_cleared() {
        let s = Store::open_in_memory().unwrap();
        let c = s.conn();
        let a = item("a1");
        insert_item(c, &a, "a.docx", 1).unwrap();
        set_missing(c, &a, true).unwrap();
        let m: i64 = c.query_row("SELECT missing FROM items WHERE id='itm_a1'", [], |r| r.get(0)).unwrap();
        assert_eq!(m, 1);
        set_missing(c, &a, false).unwrap();
        let m: i64 = c.query_row("SELECT missing FROM items WHERE id='itm_a1'", [], |r| r.get(0)).unwrap();
        assert_eq!(m, 0);
    }
}
