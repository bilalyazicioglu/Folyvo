use std::path::Path;
use std::sync::Arc;

use folyvo_core::engine::Engine;
use folyvo_core::model::{ItemId, VersionId};

use crate::dto::{ExportDto, ItemSummaryDto, RestoreDto, VaultStatsDto, VersionDto};
use crate::errors::UserError;

const MAX_NAME_CHARS: usize = 200;

/// Everything the UI can ask for. Takes and returns plain strings and DTOs, so it is tested without a window.
#[derive(Clone)]
pub struct AppApi {
    engine: Arc<Engine>,
}

impl AppApi {
    pub fn new(engine: Arc<Engine>) -> AppApi {
        AppApi { engine }
    }

    pub fn recent(&self, limit: u32) -> Result<Vec<ItemSummaryDto>, UserError> {
        Ok(self.engine.list_recent(limit)?.into_iter().map(Into::into).collect())
    }

    pub fn history(&self, item: &str) -> Result<Vec<VersionDto>, UserError> {
        let item = ItemId::parse(item)?;
        Ok(self.engine.history(&item)?.into_iter().map(Into::into).collect())
    }

    pub fn name_version(&self, version: &str, note: Option<String>) -> Result<(), UserError> {
        let version = VersionId::parse(version)?;
        let note = note.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty());
        if note.as_deref().is_some_and(|n| n.chars().count() > MAX_NAME_CHARS) {
            return Err(UserError::bad_request(
                "That name is too long",
                &format!("Names can have up to {MAX_NAME_CHARS} characters."),
            ));
        }
        Ok(self.engine.name_version(&version, note.as_deref())?)
    }

    pub fn mark_ready(&self, item: &str, version: &str) -> Result<(), UserError> {
        let (item, version) = (ItemId::parse(item)?, VersionId::parse(version)?);
        Ok(self.engine.mark_share_ready(&item, &version)?)
    }

    pub fn export(&self, item: &str, expected: &str, dest_dir: &str) -> Result<ExportDto, UserError> {
        let (item, expected) = (ItemId::parse(item)?, VersionId::parse(expected)?);
        Ok(self.engine.export(&item, &expected, Path::new(dest_dir))?.into())
    }

    pub fn restore(&self, item: &str, version: &str) -> Result<RestoreDto, UserError> {
        let (item, version) = (ItemId::parse(item)?, VersionId::parse(version)?);
        Ok(self.engine.restore(&item, &version)?.into())
    }

    pub fn stats(&self) -> Result<VaultStatsDto, UserError> {
        Ok(self.engine.vault_stats()?.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::StatusDto;
    use folyvo_core::ports::{FakeClock, SeqIdGen};
    use std::fs;
    use std::path::PathBuf;

    struct Rig {
        dir: tempfile::TempDir,
        engine: Arc<Engine>,
        api: AppApi,
    }

    fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("work")).unwrap();
        fs::create_dir(dir.path().join("out")).unwrap();
        let engine = Arc::new(
            Engine::open(
                &dir.path().join("vault"),
                Arc::new(FakeClock::new(1_000)),
                Arc::new(SeqIdGen::new()),
            )
            .unwrap(),
        );
        Rig { api: AppApi::new(engine.clone()), engine, dir }
    }

    impl Rig {
        fn save(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let p = self.dir.path().join("work").join(name);
            fs::write(&p, bytes).unwrap();
            self.engine.ingest_path(&p).unwrap();
            p
        }
        fn item(&self) -> String {
            self.api.recent(10).unwrap()[0].item_id.clone()
        }
        fn out(&self) -> String {
            self.dir.path().join("out").to_string_lossy().into_owned()
        }
    }

    #[test]
    fn recent_lists_the_newest_file_first_with_its_status() {
        let r = rig();
        r.save("a.docx", b"one");
        r.engine.ingest_path(&r.dir.path().join("work/a.docx")).unwrap();
        let recent = r.api.recent(10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].name, "a.docx");
        assert_eq!(recent[0].status, StatusDto::NotShared);
        assert_eq!(recent[0].latest_seq, 1);
    }

    #[test]
    fn history_is_newest_first_and_marks_the_ready_version() {
        let r = rig();
        let p = r.save("a.docx", b"one");
        fs::write(&p, b"two").unwrap();
        r.engine.ingest_path(&p).unwrap();
        let item = r.item();
        let v1 = r.api.history(&item).unwrap()[1].id.clone();
        r.api.mark_ready(&item, &v1).unwrap();
        let h = r.api.history(&item).unwrap();
        assert_eq!(h.iter().map(|v| v.seq).collect::<Vec<_>>(), vec![2, 1]);
        assert_eq!(h.iter().map(|v| v.is_share_ready).collect::<Vec<_>>(), vec![false, true]);
        assert_eq!(r.api.recent(10).unwrap()[0].status, StatusDto::ChangedSinceReady);
    }

    #[test]
    fn export_copies_the_version_asked_for() {
        let r = rig();
        let p = r.save("a.docx", b"one");
        let item = r.item();
        let v1 = r.api.history(&item).unwrap()[0].id.clone();
        fs::write(&p, b"two").unwrap();
        r.engine.ingest_path(&p).unwrap();

        let older = r.api.export(&item, &v1, &r.out()).unwrap();
        assert_eq!(fs::read(&older.destination).unwrap(), b"one");
    }

    #[test]
    fn restore_returns_the_new_version_number() {
        let r = rig();
        let p = r.save("a.docx", b"one");
        fs::write(&p, b"two").unwrap();
        r.engine.ingest_path(&p).unwrap();
        let item = r.item();
        let v1 = r.api.history(&item).unwrap()[1].id.clone();
        let restored = r.api.restore(&item, &v1).unwrap();
        assert_eq!(restored.seq, 3);
        assert_eq!(fs::read(&p).unwrap(), b"one");
    }

    #[test]
    fn names_are_trimmed_cleared_and_limited() {
        let r = rig();
        r.save("a.docx", b"one");
        let item = r.item();
        let v = r.api.history(&item).unwrap()[0].id.clone();
        r.api.name_version(&v, Some("  Sent to Acme  ".into())).unwrap();
        assert_eq!(r.api.history(&item).unwrap()[0].note.as_deref(), Some("Sent to Acme"));
        r.api.name_version(&v, Some("   ".into())).unwrap();
        assert_eq!(r.api.history(&item).unwrap()[0].note, None);
        let long = "x".repeat(201);
        assert_eq!(r.api.name_version(&v, Some(long)).unwrap_err().code, "bad_request");
    }

    #[test]
    fn malformed_and_unknown_ids_are_user_errors_not_panics() {
        let r = rig();
        assert_eq!(r.api.history("nonsense").unwrap_err().code, "bad_request");
        assert_eq!(r.api.history("itm_00ff").unwrap_err().code, "item_missing");
        assert_eq!(r.api.mark_ready("itm_00ff", "ver_00ff").unwrap_err().code, "item_missing");
    }

    #[test]
    fn stats_count_items_versions_and_bytes() {
        let r = rig();
        r.save("a.docx", b"12345");
        let s = r.api.stats().unwrap();
        assert_eq!((s.items, s.versions, s.bytes), (1, 1, 5));
    }
}
