use folyvo_core::engine::{EngineEvent, ItemSummary, Save, VaultStats, VersionView};
use folyvo_core::export::ExportResult;
use folyvo_core::model::{ShareEvent, ShareEventKind, Version, VersionOrigin};
use folyvo_core::which::Match;
use serde::Serialize;
use ts_rs::TS;

#[derive(Serialize, TS, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum OriginDto {
    Initial,
    Auto,
    Restore,
}

impl From<VersionOrigin> for OriginDto {
    fn from(o: VersionOrigin) -> Self {
        match o {
            VersionOrigin::Initial => OriginDto::Initial,
            VersionOrigin::Auto => OriginDto::Auto,
            VersionOrigin::Restore => OriginDto::Restore,
        }
    }
}

#[derive(Serialize, TS, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ItemSummaryDto {
    pub item_id: String,
    pub name: String,
    pub path: Option<String>,
    #[ts(type = "number")]
    pub latest_seq: i64,
    #[ts(type = "number")]
    pub latest_at: i64,
    pub missing: bool,
}

impl From<ItemSummary> for ItemSummaryDto {
    fn from(s: ItemSummary) -> Self {
        Self {
            item_id: s.item_id.to_string(),
            name: s.name,
            path: s.path,
            latest_seq: s.latest_seq,
            latest_at: s.latest_at,
            missing: s.missing,
        }
    }
}

#[derive(Serialize, TS, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VersionDto {
    pub id: String,
    #[ts(type = "number")]
    pub seq: i64,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub size: u64,
    pub content_hash: String,
    pub origin: OriginDto,
    pub note: Option<String>,
    pub code: String,
}

impl From<VersionView> for VersionDto {
    fn from(v: VersionView) -> Self {
        Self {
            id: v.id.to_string(),
            seq: v.seq,
            created_at: v.created_at,
            size: v.size,
            content_hash: v.content_hash.to_string(),
            origin: v.origin.into(),
            note: v.note,
            code: v.code,
        }
    }
}

#[derive(Serialize, TS, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VaultStatsDto {
    #[ts(type = "number")]
    pub items: u64,
    #[ts(type = "number")]
    pub versions: u64,
    #[ts(type = "number")]
    pub bytes: u64,
}

impl From<VaultStats> for VaultStatsDto {
    fn from(s: VaultStats) -> Self {
        Self { items: s.items, versions: s.versions, bytes: s.bytes }
    }
}

#[derive(Serialize, TS, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExportDto {
    pub destination: String,
    pub version_id: String,
}

impl From<ExportResult> for ExportDto {
    fn from(r: ExportResult) -> Self {
        Self {
            destination: r.destination.to_string_lossy().into_owned(),
            version_id: r.version_id.to_string(),
        }
    }
}

#[derive(Serialize, TS, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RestoreDto {
    pub version_id: String,
    #[ts(type = "number")]
    pub seq: i64,
}

impl From<Version> for RestoreDto {
    fn from(v: Version) -> Self {
        Self { version_id: v.id.to_string(), seq: v.seq }
    }
}

#[derive(Serialize, TS, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum EngineEventDto {
    VersionCreated {
        item_id: String,
        version_id: String,
    },
    Exported {
        item_id: String,
        destination: String,
    },
    Handed {
        item_id: String,
        version_id: String,
    },
    ItemMissing {
        item_id: String,
    },
    ItemUpdated {
        item_id: String,
    },
    RootsChanged,
    /// The event channel lagged: everything the UI shows may be stale.
    Resync,
}

impl From<EngineEvent> for EngineEventDto {
    fn from(e: EngineEvent) -> Self {
        match e {
            EngineEvent::VersionCreated { item, version } => {
                EngineEventDto::VersionCreated { item_id: item.to_string(), version_id: version.to_string() }
            }
            EngineEvent::Exported { item, destination } => EngineEventDto::Exported {
                item_id: item.to_string(),
                destination: destination.to_string_lossy().into_owned(),
            },
            EngineEvent::Handed { item, version } => {
                EngineEventDto::Handed { item_id: item.to_string(), version_id: version.to_string() }
            }
            EngineEvent::ItemMissing { item } => EngineEventDto::ItemMissing { item_id: item.to_string() },
            EngineEvent::ItemUpdated { item } => EngineEventDto::ItemUpdated { item_id: item.to_string() },
            EngineEvent::RootsChanged => EngineEventDto::RootsChanged,
        }
    }
}

#[derive(Serialize, TS, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FolderDto {
    pub path: String,
    /// Set when the folder cannot be watched right now (gone, unmounted, not allowed).
    pub problem: Option<crate::errors::UserError>,
}

/// How a copy left: saved to a folder through Folyvo, or handed to another app.
#[derive(Serialize, TS, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum CopyKind {
    Saved,
    Handed,
}

#[derive(Serialize, TS, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CopyDto {
    pub item_id: String,
    pub version_id: String,
    pub kind: CopyKind,
    pub service: Option<String>,
    pub target_path: Option<String>,
    #[ts(type = "number")]
    pub at: i64,
}

impl From<ShareEvent> for CopyDto {
    fn from(e: ShareEvent) -> Self {
        Self {
            item_id: e.item_id.to_string(),
            version_id: e.version_id.to_string(),
            kind: match e.kind {
                ShareEventKind::Exported => CopyKind::Saved,
                ShareEventKind::Handed => CopyKind::Handed,
            },
            service: e.service,
            target_path: e.target_path,
            at: e.at,
        }
    }
}

#[derive(Serialize, TS, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SaveDto {
    pub item_id: String,
    pub version_id: String,
    #[ts(type = "number")]
    pub at: i64,
}

impl From<Save> for SaveDto {
    fn from(s: Save) -> Self {
        Self { item_id: s.item_id.to_string(), version_id: s.version_id.to_string(), at: s.at }
    }
}

/// Saves and copies in a time range (the popover asks for today).
#[derive(Serialize, TS, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ActivityDto {
    pub saves: Vec<SaveDto>,
    pub copies: Vec<CopyDto>,
}

/// One saved version whose content is exactly a given file.
#[derive(Serialize, TS, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MatchDto {
    pub item_id: String,
    pub name: String,
    pub version: VersionDto,
    pub copies: Vec<CopyDto>,
}

impl From<Match> for MatchDto {
    fn from(m: Match) -> Self {
        Self {
            item_id: m.item_id.to_string(),
            name: m.name,
            version: m.version.into(),
            copies: m.events.into_iter().map(Into::into).collect(),
        }
    }
}

/// What the versions window shows for one version.
#[derive(Serialize, TS, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PreviewDto {
    /// `data:image/png;base64,...`, or none when Quick Look cannot draw this kind of file.
    pub picture: Option<String>,
    /// The app that opens this kind of file, such as "Microsoft Word".
    pub app_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use folyvo_core::engine::EngineEvent;
    use folyvo_core::engine::VersionView;
    use folyvo_core::model::{ContentHash, ItemId, VersionId, VersionOrigin};
    use serde_json::json;

    #[test]
    fn a_version_keeps_every_field_the_ui_needs() {
        let v = VersionView {
            id: VersionId::from_suffix("0a"),
            seq: 3,
            created_at: 1_700_000_000_000,
            size: 2048,
            content_hash: ContentHash::of_bytes(b"x"),
            origin: VersionOrigin::Restore,
            note: Some("Sent to Acme".into()),
            code: "0a".into(),
        };
        let json = serde_json::to_value(VersionDto::from(v)).unwrap();
        assert_eq!(json["id"], "ver_0a");
        assert_eq!(json["seq"], 3);
        assert_eq!(json["createdAt"], 1_700_000_000_000i64);
        assert_eq!(json["origin"], "restore");
        assert_eq!(json["note"], "Sent to Acme");
        assert_eq!(json["code"], "0a");
    }

    #[test]
    fn events_are_tagged_by_kind() {
        let e = EngineEventDto::from(EngineEvent::VersionCreated {
            item: ItemId::from_suffix("01"),
            version: VersionId::from_suffix("02"),
        });
        assert_eq!(
            serde_json::to_value(e).unwrap(),
            json!({"kind": "versionCreated", "itemId": "itm_01", "versionId": "ver_02"})
        );
        assert_eq!(
            serde_json::to_value(EngineEventDto::from(EngineEvent::RootsChanged)).unwrap(),
            json!({"kind": "rootsChanged"})
        );
        assert_eq!(serde_json::to_value(EngineEventDto::Resync).unwrap(), json!({"kind": "resync"}));
        let handed = EngineEventDto::from(EngineEvent::Handed {
            item: ItemId::from_suffix("01"),
            version: VersionId::from_suffix("02"),
        });
        assert_eq!(
            serde_json::to_value(handed).unwrap(),
            json!({"kind": "handed", "itemId": "itm_01", "versionId": "ver_02"})
        );
    }

    #[test]
    fn a_copy_serialises_its_kind_in_camel_case() {
        use folyvo_core::model::{ShareEvent, ShareEventKind};
        let handed = CopyDto::from(ShareEvent {
            item_id: ItemId::from_suffix("01"),
            version_id: VersionId::from_suffix("02"),
            kind: ShareEventKind::Handed,
            service: Some("mail".into()),
            target_path: None,
            at: 5,
        });
        assert_eq!(
            serde_json::to_value(handed).unwrap(),
            json!({"itemId": "itm_01", "versionId": "ver_02", "kind": "handed", "service": "mail", "targetPath": null, "at": 5})
        );
        assert_eq!(serde_json::to_value(CopyKind::Saved).unwrap(), json!("saved"));
    }
}
