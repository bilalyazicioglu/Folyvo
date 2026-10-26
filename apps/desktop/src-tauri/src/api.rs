use std::path::Path;
use std::sync::Arc;

use folyvo_core::engine::Engine;
use folyvo_core::model::{ItemId, VersionId};

use base64::Engine as _;

use crate::dto::{
    ActivityDto, CopyDto, ExportDto, ItemSummaryDto, MatchDto, PreviewDto, RestoreDto, VaultStatsDto,
    VersionDto,
};
use crate::errors::UserError;
use crate::preview;

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

    /// Copy `version` into `dest_dir`.
    pub fn export(&self, item: &str, version: &str, dest_dir: &str) -> Result<ExportDto, UserError> {
        let (item, version) = (ItemId::parse(item)?, VersionId::parse(version)?);
        Ok(self.engine.export(&item, &version, Path::new(dest_dir))?.into())
    }

    /// Saves and copies with `from <= time < to` (unix milliseconds).
    pub fn activity(&self, from: i64, to: i64) -> Result<ActivityDto, UserError> {
        let (saves, copies) = self.engine.activity(from, to)?;
        Ok(ActivityDto {
            saves: saves.into_iter().map(Into::into).collect(),
            copies: copies.into_iter().map(Into::into).collect(),
        })
    }

    /// Copies of an item's versions, oldest first.
    pub fn copies(&self, item: &str) -> Result<Vec<CopyDto>, UserError> {
        let item = ItemId::parse(item)?;
        Ok(self.engine.events(&item)?.into_iter().map(Into::into).collect())
    }

    /// The item Folyvo tracks at `path`, if any.
    pub fn item_at(&self, path: &str) -> Result<Option<String>, UserError> {
        Ok(self.engine.item_at(Path::new(path))?.map(|i| i.to_string()))
    }

    /// Saved versions whose content is exactly the file at `path`.
    pub fn which(&self, path: &str) -> Result<Vec<MatchDto>, UserError> {
        Ok(self.engine.which_one(Path::new(path))?.into_iter().map(Into::into).collect())
    }

    pub fn restore(&self, item: &str, version: &str) -> Result<RestoreDto, UserError> {
        let (item, version) = (ItemId::parse(item)?, VersionId::parse(version)?);
        Ok(self.engine.restore(&item, &version)?.into())
    }

    /// A picture of `version` and the app that opens it. The picture is drawn from a read-only copy.
    pub fn preview(&self, item: &str, version: &str) -> Result<PreviewDto, UserError> {
        let (item, version) = (ItemId::parse(item)?, VersionId::parse(version)?);
        let copy = self.engine.materialise(&item, &version)?;
        let key = self
            .engine
            .history(&item)?
            .into_iter()
            .find(|v| v.id == version)
            .map(|v| v.content_hash.to_string())
            .ok_or(folyvo_core::CoreError::VersionMissing(version.clone()))?;
        let cache = std::env::temp_dir().join("folyvo-previews");
        let picture =
            preview::thumbnail(&copy, &cache, &key).and_then(|png| std::fs::read(png).ok()).map(|bytes| {
                format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
            });
        Ok(PreviewDto { picture, app_name: preview::default_app_name(&copy) })
    }

    /// Open the working file (`newest`, when it exists) or a read-only copy of `version` in its app.
    pub fn open_version(&self, item: &str, version: &str, newest: bool) -> Result<(), UserError> {
        let (item_id, version) = (ItemId::parse(item)?, VersionId::parse(version)?);
        let working = self
            .engine
            .list_recent(u32::MAX)?
            .into_iter()
            .find(|i| i.item_id == item_id)
            .and_then(|i| i.path)
            .map(std::path::PathBuf::from)
            .filter(|p| newest && p.is_file());
        let target = match working {
            Some(p) => p,
            None => self.engine.materialise(&item_id, &version)?,
        };
        preview::open_in_app(&target).map_err(|_| UserError::internal())
    }

    pub fn stats(&self) -> Result<VaultStatsDto, UserError> {
        Ok(self.engine.vault_stats()?.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::CopyKind;
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
    fn activity_lists_todays_saves_and_copies() {
        let r = rig();
        r.save("a.docx", b"one");
        let item = r.item();
        let v = r.api.history(&item).unwrap()[0].id.clone();
        r.engine
            .record_handed(&ItemId::parse(&item).unwrap(), &VersionId::parse(&v).unwrap(), "mail")
            .unwrap();
        let a = r.api.activity(0, 2_000).unwrap();
        assert_eq!(a.saves.len(), 1);
        assert_eq!(
            (a.saves[0].item_id.as_str(), a.saves[0].version_id.as_str(), a.saves[0].at),
            (item.as_str(), v.as_str(), 1_000)
        );
        assert_eq!(a.copies.len(), 1);
        assert_eq!(a.copies[0].service.as_deref(), Some("mail"));
        assert!(r.api.activity(2_000, 3_000).unwrap().saves.is_empty());
    }

    #[test]
    fn copies_lists_saved_and_handed_copies_in_order() {
        let r = rig();
        r.save("a.docx", b"one");
        let item = r.item();
        let v = r.api.history(&item).unwrap()[0].id.clone();
        r.api.export(&item, &v, &r.out()).unwrap();
        r.engine
            .record_handed(&ItemId::parse(&item).unwrap(), &VersionId::parse(&v).unwrap(), "airdrop")
            .unwrap();
        let c = r.api.copies(&item).unwrap();
        assert_eq!(c.iter().map(|c| c.kind).collect::<Vec<_>>(), vec![CopyKind::Saved, CopyKind::Handed]);
        assert!(c[0].target_path.is_some() && c[1].service.as_deref() == Some("airdrop"));
    }

    #[test]
    fn item_at_names_a_tracked_file() {
        let r = rig();
        let p = r.save("a.docx", b"one");
        assert_eq!(r.api.item_at(p.to_str().unwrap()).unwrap(), Some(r.item()));
        assert_eq!(r.api.item_at(r.dir.path().join("nope.docx").to_str().unwrap()).unwrap(), None);
    }

    #[test]
    fn which_finds_a_copy_and_says_nothing_for_an_edited_one() {
        let r = rig();
        r.save("Teklif.docx", b"one");
        let copy = r.dir.path().join("back.docx");
        fs::write(&copy, b"one").unwrap();
        let m = r.api.which(copy.to_str().unwrap()).unwrap();
        assert_eq!((m.len(), m[0].name.as_str()), (1, "Teklif.docx"));
        fs::write(&copy, b"one, edited").unwrap();
        assert!(r.api.which(copy.to_str().unwrap()).unwrap().is_empty());
        assert_eq!(r.api.which(r.dir.path().to_str().unwrap()).unwrap_err().code, "not_a_file");
        assert_eq!(
            r.api.which(r.dir.path().join("nope").to_str().unwrap()).unwrap_err().code,
            "file_unavailable"
        );
    }

    #[test]
    fn export_takes_the_version_to_copy() {
        let r = rig();
        r.save("a.docx", b"one");
        let item = r.item();
        let v = r.api.history(&item).unwrap()[0].id.clone();
        let out = r.api.export(&item, &v, &r.out()).unwrap();
        assert_eq!(out.version_id, v);
    }

    #[test]
    fn preview_of_an_unknown_version_is_a_user_error() {
        let r = rig();
        r.save("a.docx", b"one");
        assert_eq!(r.api.preview(&r.item(), "ver_00ff").unwrap_err().code, "version_missing");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn preview_of_a_saved_text_file_has_a_picture_and_an_app() {
        let r = rig();
        r.save("notlar.txt", "Toplantı notları\nfiyatlar güncellendi\n".as_bytes());
        let item = r.item();
        let v = r.api.history(&item).unwrap()[0].id.clone();
        let p = r.api.preview(&item, &v).unwrap();
        assert!(
            p.picture.as_deref().is_some_and(|d| d.starts_with("data:image/png;base64,")),
            "{:?}",
            p.picture.map(|d| d.len())
        );
        assert!(p.app_name.is_some());
    }

    #[test]
    fn recent_lists_the_newest_file_first() {
        let r = rig();
        r.save("a.docx", b"one");
        r.engine.ingest_path(&r.dir.path().join("work/a.docx")).unwrap();
        let recent = r.api.recent(10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].name, "a.docx");
        assert_eq!(recent[0].latest_seq, 1);
    }

    #[test]
    fn history_is_newest_first() {
        let r = rig();
        let p = r.save("a.docx", b"one");
        fs::write(&p, b"two").unwrap();
        r.engine.ingest_path(&p).unwrap();
        let item = r.item();
        let h = r.api.history(&item).unwrap();
        assert_eq!(h.iter().map(|v| v.seq).collect::<Vec<_>>(), vec![2, 1]);
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
        assert_eq!(r.api.export("itm_00ff", "ver_00ff", &r.out()).unwrap_err().code, "item_missing");
    }

    #[test]
    fn stats_count_items_versions_and_bytes() {
        let r = rig();
        r.save("a.docx", b"12345");
        let s = r.api.stats().unwrap();
        assert_eq!((s.items, s.versions, s.bytes), (1, 1, 5));
    }
}
