pub(crate) mod repo;

use std::path::Path;

use rusqlite::Connection;

use crate::error::{CoreError, Result};

pub const SCHEMA_VERSION: i64 = 2;
const SCHEMA: &str = include_str!("schema.sql");

/// v1 -> v2: the share-ready mark is gone, copies handed to another app are recorded.
/// Exported events are kept; marked events and the pointer table are dropped.
const MIGRATE_1_TO_2: &str = "
CREATE TABLE share_events_v2 (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id     TEXT    NOT NULL REFERENCES items(id),
    version_id  TEXT    NOT NULL,
    kind        TEXT    NOT NULL CHECK (kind IN ('exported', 'handed')),
    service     TEXT,
    target_path TEXT,
    target_key  TEXT,
    at          INTEGER NOT NULL,
    CHECK ((kind = 'handed') = (service IS NOT NULL)),
    FOREIGN KEY (version_id, item_id) REFERENCES versions(id, item_id)
);
INSERT INTO share_events_v2 (id, item_id, version_id, kind, service, target_path, target_key, at)
    SELECT id, item_id, version_id, kind, NULL, target_path, target_key, at FROM share_events WHERE kind = 'exported';
DROP TABLE share_events;
ALTER TABLE share_events_v2 RENAME TO share_events;
DROP TABLE share_ready;
CREATE INDEX versions_by_hash ON versions(content_hash);
";

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        let conn = Connection::open(path)?;
        conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Store> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        match version {
            0 => {
                conn.execute_batch(SCHEMA)?;
                conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            }
            1 => {
                let tx = conn.transaction()?;
                tx.execute_batch(MIGRATE_1_TO_2)?;
                tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                tx.commit()?;
            }
            SCHEMA_VERSION => {}
            other => return Err(CoreError::Corrupt(format!("unsupported database schema version {other}"))),
        }
        Ok(Store { conn })
    }

    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    pub(crate) fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::open_in_memory().unwrap()
    }

    fn exec(s: &Store, sql: &str) -> rusqlite::Result<usize> {
        s.conn().execute(sql, [])
    }

    fn seed_item(s: &Store, id: &str) {
        exec(s, &format!("INSERT INTO items(id, display_name, created_at) VALUES ('{id}', 'n', 1)")).unwrap();
    }

    fn seed_object(s: &Store, hash: &str) {
        exec(
            s,
            &format!("INSERT OR IGNORE INTO objects(content_hash, size, stored_at) VALUES ('{hash}', 1, 1)"),
        )
        .unwrap();
    }

    fn seed_version(s: &Store, id: &str, item: &str, seq: i64) -> rusqlite::Result<usize> {
        seed_object(s, "b3_x");
        exec(
            s,
            &format!(
                "INSERT INTO versions(id, item_id, seq, content_hash, size, created_at, origin) \
             VALUES ('{id}', '{item}', {seq}, 'b3_x', 1, 1, 'auto')"
            ),
        )
    }

    #[test]
    fn open_creates_every_table() {
        let s = store();
        let names: Vec<String> = s.conn()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap().query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
        for t in ["items", "locations", "objects", "share_events", "versions", "watched_roots"] {
            assert!(names.contains(&t.to_string()), "missing table {t}: {names:?}");
        }
    }

    #[test]
    fn reopening_a_file_database_is_idempotent_and_uses_wal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("folyvo.db");
        drop(Store::open(&path).unwrap());
        let s = Store::open(&path).unwrap();
        let v: i64 = s.conn().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        let mode: String = s.conn().query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[test]
    fn a_database_from_a_newer_schema_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("folyvo.db");
        {
            let c = rusqlite::Connection::open(&path).unwrap();
            c.pragma_update(None, "user_version", 99).unwrap();
        }
        assert!(matches!(Store::open(&path), Err(CoreError::Corrupt(_))));
    }

    #[test]
    fn version_for_unknown_item_is_rejected_by_foreign_key() {
        let s = store();
        assert!(seed_version(&s, "ver_1", "itm_ghost", 1).is_err());
    }

    #[test]
    fn duplicate_seq_for_one_item_is_rejected() {
        let s = store();
        seed_item(&s, "itm_a");
        seed_version(&s, "ver_1", "itm_a", 1).unwrap();
        assert!(seed_version(&s, "ver_2", "itm_a", 1).is_err());
        seed_version(&s, "ver_3", "itm_a", 2).unwrap();
    }

    #[test]
    fn unknown_origin_is_rejected() {
        let s = store();
        seed_item(&s, "itm_a");
        seed_object(&s, "b3_x");
        assert!(exec(
            &s,
            "INSERT INTO versions(id,item_id,seq,content_hash,size,created_at,origin) \
            VALUES ('ver_1','itm_a',1,'b3_x',1,1,'bogus')"
        )
        .is_err());
    }

    #[test]
    fn only_one_current_location_per_item() {
        let s = store();
        seed_item(&s, "itm_a");
        exec(&s, "INSERT INTO locations(item_id,path,canonical_key,observed_at,is_current) VALUES ('itm_a','/a','k1',1,1)").unwrap();
        assert!(exec(&s, "INSERT INTO locations(item_id,path,canonical_key,observed_at,is_current) VALUES ('itm_a','/b','k2',2,1)").is_err());
        exec(&s, "INSERT INTO locations(item_id,path,canonical_key,observed_at,is_current) VALUES ('itm_a','/old','k0',0,0)").unwrap();
    }

    #[test]
    fn only_one_current_item_per_canonical_path() {
        let s = store();
        seed_item(&s, "itm_a");
        seed_item(&s, "itm_b");
        exec(&s, "INSERT INTO locations(item_id,path,canonical_key,observed_at,is_current) VALUES ('itm_a','/a','k1',1,1)").unwrap();
        assert!(exec(&s, "INSERT INTO locations(item_id,path,canonical_key,observed_at,is_current) VALUES ('itm_b','/a','k1',1,1)").is_err());
        exec(&s, "INSERT INTO locations(item_id,path,canonical_key,observed_at,is_current) VALUES ('itm_b','/a','k1',1,0)").unwrap();
    }

    const V1: &str = include_str!("schema_v1.sql");

    /// A v1 vault as the demo left it: one item, three versions over two objects, a share-ready
    /// pointer, one marked and one exported event.
    fn v1_vault(path: &std::path::Path) {
        let c = rusqlite::Connection::open(path).unwrap();
        c.execute_batch(V1).unwrap();
        c.execute_batch(
            "INSERT INTO items(id,display_name,created_at) VALUES ('itm_a','a.docx',1);
             INSERT INTO objects(content_hash,size,stored_at) VALUES ('b3_x',1,1),('b3_y',1,1);
             INSERT INTO versions(id,item_id,seq,content_hash,size,created_at,origin) VALUES
               ('ver_1','itm_a',1,'b3_x',1,1,'initial'),
               ('ver_2','itm_a',2,'b3_y',1,2,'auto'),
               ('ver_3','itm_a',3,'b3_x',1,3,'restore');
             INSERT INTO share_ready(item_id,version_id,marked_at) VALUES ('itm_a','ver_2',4);
             INSERT INTO share_events(item_id,version_id,kind,target_path,target_key,at) VALUES
               ('itm_a','ver_2','marked',NULL,NULL,4),
               ('itm_a','ver_2','exported','/out/a.docx','/out/a.docx',5);",
        )
        .unwrap();
        c.pragma_update(None, "user_version", 1).unwrap();
    }

    fn count(s: &Store, table: &str) -> i64 {
        s.conn().query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
    }

    fn has(s: &Store, kind: &str, name: &str) -> bool {
        s.conn()
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type=?1 AND name=?2", [kind, name], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap()
            == 1
    }

    #[test]
    fn a_new_database_is_v2_without_share_ready() {
        let s = store();
        let v: i64 = s.conn().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 2);
        assert!(!has(&s, "table", "share_ready"));
        assert!(has(&s, "index", "versions_by_hash"));
    }

    #[test]
    fn a_v1_database_is_migrated_and_keeps_every_version_object_and_export() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("folyvo.db");
        v1_vault(&path);
        let s = Store::open(&path).unwrap();
        let v: i64 = s.conn().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 2);
        assert_eq!((count(&s, "versions"), count(&s, "objects"), count(&s, "share_events")), (3, 2, 1));
        let (kind, service, target): (String, Option<String>, String) = s
            .conn()
            .query_row("SELECT kind, service, target_path FROM share_events", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!((kind.as_str(), service, target.as_str()), ("exported", None, "/out/a.docx"));
        assert!(!has(&s, "table", "share_ready"));
        assert!(has(&s, "index", "versions_by_hash"));
    }

    #[test]
    fn migrating_twice_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("folyvo.db");
        v1_vault(&path);
        drop(Store::open(&path).unwrap());
        let s = Store::open(&path).unwrap();
        assert_eq!((count(&s, "versions"), count(&s, "objects"), count(&s, "share_events")), (3, 2, 1));
    }

    #[test]
    fn a_handed_event_needs_a_service_and_an_exported_one_must_not_have_one() {
        let s = store();
        seed_item(&s, "itm_a");
        seed_version(&s, "ver_a1", "itm_a", 1).unwrap();
        let ins = |kind: &str, service: &str| {
            exec(&s, &format!("INSERT INTO share_events(item_id,version_id,kind,service,at) VALUES ('itm_a','ver_a1','{kind}',{service},1)"))
        };
        assert!(ins("handed", "NULL").is_err());
        assert!(ins("exported", "'mail'").is_err());
        ins("handed", "'mail'").unwrap();
        ins("exported", "NULL").unwrap();
    }
}
