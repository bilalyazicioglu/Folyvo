use std::path::Path;
use std::sync::Arc;

use folyvo_core::engine::Engine;
use folyvo_core::model::{ItemId, VersionId};

use base64::Engine as _;

use crate::dto::{
    ActivityDto, CopyDto, ExportDto, ItemSummaryDto, MarkedPageDto, MatchDto, PreviewDto, RestoreDto,
    VaultStatsDto, VersionDto,
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

    pub fn item(&self, item: &str) -> Result<ItemSummaryDto, UserError> {
        Ok(self.engine.item(&ItemId::parse(item)?)?.into())
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

    /// A read-only copy of `version` under the file's own name, for the share sheet.
    pub fn share_file(&self, item: &str, version: &str) -> Result<std::path::PathBuf, UserError> {
        let (item, version) = (ItemId::parse(item)?, VersionId::parse(version)?);
        Ok(self.engine.materialise(&item, &version)?)
    }

    /// Remembers that `service` took a copy of `version` from the share sheet.
    pub fn record_share(&self, item: &str, version: &str, service: &str) -> Result<CopyDto, UserError> {
        let (item, version) = (ItemId::parse(item)?, VersionId::parse(version)?);
        Ok(self.engine.record_handed(&item, &version, service)?.into())
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
        let app_name = preview::default_app_name(&copy);
        let reveals_copy = preview::how_to_open(&copy, true) == preview::Opening::Reveal;
        if let Some(text) = preview::text_excerpt(&copy) {
            return Ok(PreviewDto { picture: None, text: Some(text), reveals_copy, app_name });
        }
        let cache = std::env::temp_dir().join("folyvo-previews");
        let picture =
            preview::thumbnail(&copy, &cache, &key).and_then(|png| std::fs::read(png).ok()).map(|bytes| {
                format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
            });
        Ok(PreviewDto { picture, text: None, reveals_copy, app_name })
    }

    /// `version`'s text with what changed since the save before marked. The first save comes
    /// back with nothing marked. Worked out each time it is asked for, and never kept.
    pub fn changes(&self, item: &str, version: &str) -> Result<MarkedPageDto, UserError> {
        let (item, version) = (ItemId::parse(item)?, VersionId::parse(version)?);
        let inputs = self.engine.compare_inputs(&version)?;
        if inputs.item != item {
            return Err(folyvo_core::CoreError::VersionMissing(version).into());
        }
        let after = crate::changes::reading_of(&inputs.after, &inputs.name);
        let changes = match &inputs.before {
            None => folyvo_compare::compare(&after, &after),
            Some((_, _, hash)) if *hash == inputs.after_hash => return Ok(MarkedPageDto::Identical),
            Some((_, path, _)) if !folyvo_compare::same_kind(path, &inputs.after, &inputs.name) => {
                return Ok(MarkedPageDto::Unreadable)
            }
            Some((_, path, _)) => {
                folyvo_compare::compare(&crate::changes::reading_of(path, &inputs.name), &after)
            }
        };
        Ok(changes.marked.into())
    }

    /// Open the working file (`newest`, when it exists) or a read-only copy of `version` in its app.
    pub fn open_version(&self, item: &str, version: &str, newest: bool) -> Result<(), UserError> {
        let (item_id, version) = (ItemId::parse(item)?, VersionId::parse(version)?);
        let working =
            self.engine.item(&item_id)?.path.map(std::path::PathBuf::from).filter(|p| newest && p.is_file());
        let (target, copy) = match working {
            Some(p) => (p, false),
            None => (self.engine.materialise(&item_id, &version)?, true),
        };
        preview::open_in_app(&target, preview::how_to_open(&target, copy)).map_err(|_| UserError::internal())
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
    fn item_names_one_file_by_its_id() {
        let r = rig();
        r.save("Teklif.docx", b"one");
        let i = r.api.item(&r.item()).unwrap();
        assert_eq!((i.name.as_str(), i.missing), ("Teklif.docx", false));
        assert!(r.api.item("itm_0f0f").is_err());
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
    fn preview_says_when_a_copy_is_shown_in_finder_instead_of_opened() {
        let r = rig();
        r.save("Run me.command", b"echo hi\n");
        let item = r.item();
        let v = r.api.history(&item).unwrap()[0].id.clone();
        assert!(r.api.preview(&item, &v).unwrap().reveals_copy);
        let r = rig();
        r.save("teklif.txt", b"Teklif\n");
        let item = r.item();
        let v = r.api.history(&item).unwrap()[0].id.clone();
        assert!(!r.api.preview(&item, &v).unwrap().reveals_copy);
    }

    #[test]
    fn a_share_is_recorded_once_as_attached_in_mail() {
        let r = rig();
        r.save("teklif.docx", b"one");
        let item = r.item();
        let v = r.api.history(&item).unwrap()[0].id.clone();
        let c = r.api.record_share(&item, &v, "mail").unwrap();
        assert_eq!((c.kind, c.service.as_deref()), (crate::dto::CopyKind::Handed, Some("mail")));
        assert_eq!(r.api.copies(&item).unwrap().len(), 1);
        assert_eq!(r.api.record_share(&item, &v, "fax").unwrap_err().code, "bad_request");
    }

    #[test]
    fn preview_of_an_unknown_version_is_a_user_error() {
        let r = rig();
        r.save("a.docx", b"one");
        assert_eq!(r.api.preview(&r.item(), "ver_00ff").unwrap_err().code, "version_missing");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn preview_of_a_saved_text_file_is_its_words_and_an_app() {
        let r = rig();
        r.save("notlar.txt", "Toplantı notları\nfiyatlar güncellendi\n".as_bytes());
        let item = r.item();
        let v = r.api.history(&item).unwrap()[0].id.clone();
        let p = r.api.preview(&item, &v).unwrap();
        assert_eq!(p.text.as_deref(), Some("Toplantı notları\nfiyatlar güncellendi\n"));
        assert_eq!(p.picture, None, "plain text is never a Quick Look picture");
        assert!(p.app_name.is_some());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn preview_of_a_saved_docx_is_a_picture() {
        let r = rig();
        let dir = tempfile::tempdir().unwrap();
        let txt = dir.path().join("t.txt");
        std::fs::write(&txt, "Teklif\n").unwrap();
        let docx = dir.path().join("t.docx");
        let ok = std::process::Command::new("textutil")
            .args(["-convert", "docx", "-output"])
            .arg(&docx)
            .arg(&txt)
            .status()
            .unwrap();
        assert!(ok.success());
        r.save("teklif.docx", &std::fs::read(&docx).unwrap());
        let item = r.item();
        let v = r.api.history(&item).unwrap()[0].id.clone();
        let p = r.api.preview(&item, &v).unwrap();
        assert!(p.picture.as_deref().is_some_and(|d| d.starts_with("data:image/png;base64,")));
        assert_eq!(p.text, None);
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

    impl Rig {
        /// Saves the file at `src` as `name` in the work folder, and returns (item, version).
        fn save_file(&self, name: &str, src: &std::path::Path) -> (String, String) {
            let p = self.dir.path().join("work").join(name);
            fs::copy(src, &p).unwrap();
            let v = self.engine.ingest_path(&p).unwrap().unwrap().version().clone();
            (v.item_id.to_string(), v.id.to_string())
        }
    }

    #[test]
    fn a_word_save_comes_back_marked() {
        use crate::changes::tests::docx;
        let r = rig();
        let src = tempfile::tempdir().unwrap();
        r.save_file("P.docx", &docx(src.path(), "a", &["Proposal", "Support, 12 months"]));
        let (item, v2) = r.save_file("P.docx", &docx(src.path(), "b", &["Proposal", "Support, 18 months"]));
        let json = serde_json::to_value(r.api.changes(&item, &v2).unwrap()).unwrap();
        assert_eq!(json["kind"], "text");
        assert_eq!(json["blocks"][0]["mark"], "same");
        let changed = &json["blocks"][1];
        assert_eq!(changed["mark"], "changed");
        assert_eq!(changed["style"], serde_json::json!({"kind": "body"}));
        assert_eq!(changed["section"], "body");
        let spans = changed["spans"].as_array().unwrap();
        assert!(spans.contains(&serde_json::json!({"kind": "removed", "text": "12"})), "{spans:?}");
        assert!(spans.contains(&serde_json::json!({"kind": "added", "text": "18"})), "{spans:?}");
    }

    #[test]
    fn a_spreadsheet_save_lists_changed_cells() {
        use crate::changes::tests::xlsx;
        let r = rig();
        let src = tempfile::tempdir().unwrap();
        r.save_file("C.xlsx", &xlsx(src.path(), "a", &[("C7", "1800")]));
        let (item, v2) = r.save_file("C.xlsx", &xlsx(src.path(), "b", &[("C7", "2400")]));
        let json = serde_json::to_value(r.api.changes(&item, &v2).unwrap()).unwrap();
        assert_eq!(json["kind"], "sheets");
        assert_eq!(json["sheets"][0]["name"], "Costs");
        assert_eq!(
            json["sheets"][0]["cells"][0],
            serde_json::json!({"at": "C7", "before": "1800", "after": "2400"})
        );
    }

    #[test]
    fn an_unreadable_save_says_so() {
        let r = rig();
        r.save("photo.heic", b"one");
        r.save("photo.heic", b"two");
        let item = r.item();
        let v = r.api.history(&item).unwrap()[0].id.clone();
        assert_eq!(
            serde_json::to_value(r.api.changes(&item, &v).unwrap()).unwrap(),
            serde_json::json!({"kind": "unreadable"})
        );
    }

    #[test]
    fn the_first_save_is_all_same() {
        let r = rig();
        r.save("n.txt", b"one\n\ntwo\n");
        let item = r.item();
        let v = r.api.history(&item).unwrap()[0].id.clone();
        let json = serde_json::to_value(r.api.changes(&item, &v).unwrap()).unwrap();
        let marks: Vec<&str> =
            json["blocks"].as_array().unwrap().iter().map(|b| b["mark"].as_str().unwrap()).collect();
        assert_eq!(marks, vec!["same", "same"]);
    }

    #[test]
    fn a_save_of_another_file_is_refused() {
        let r = rig();
        let src = tempfile::tempdir().unwrap();
        let (a, _) = r.save_file("a.txt", &{
            let p = src.path().join("a");
            fs::write(&p, b"a").unwrap();
            p
        });
        let (_, b_version) = r.save_file("b.txt", &{
            let p = src.path().join("b");
            fs::write(&p, b"b").unwrap();
            p
        });
        assert_eq!(r.api.changes(&a, &b_version).unwrap_err().code, "version_missing");
    }
}
