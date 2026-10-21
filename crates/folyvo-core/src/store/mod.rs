pub(crate) mod repo;

use std::path::Path;

use rusqlite::Connection;

use crate::error::{CoreError, Result};

pub const SCHEMA_VERSION: i64 = 1;
const SCHEMA: &str = include_str!("schema.sql");

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

    fn init(conn: Connection) -> Result<Store> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        match version {
            0 => {
                conn.execute_batch(SCHEMA)?;
                conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
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
        for t in ["items", "locations", "objects", "share_events", "share_ready", "versions", "watched_roots"]
        {
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

    #[test]
    fn share_ready_cannot_point_at_another_items_version() {
        let s = store();
        seed_item(&s, "itm_a");
        seed_item(&s, "itm_b");
        seed_version(&s, "ver_a1", "itm_a", 1).unwrap();
        assert!(exec(
            &s,
            "INSERT INTO share_ready(item_id,version_id,marked_at) VALUES ('itm_b','ver_a1',1)"
        )
        .is_err());
        exec(&s, "INSERT INTO share_ready(item_id,version_id,marked_at) VALUES ('itm_a','ver_a1',1)")
            .unwrap();
    }
}
