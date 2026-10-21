use folyvo_core::engine::{EngineEvent, ItemSummary, VaultStats, VersionView};
use folyvo_core::export::ExportResult;
use folyvo_core::model::{ItemStatus, Version, VersionOrigin};
use serde::Serialize;
use ts_rs::TS;

#[derive(Serialize, TS, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum StatusDto {
    NotShared,
    Ready,
    ChangedSinceReady,
}

impl From<ItemStatus> for StatusDto {
    fn from(s: ItemStatus) -> Self {
        match s {
            ItemStatus::NotShared => StatusDto::NotShared,
            ItemStatus::Ready => StatusDto::Ready,
            ItemStatus::ChangedSinceReady => StatusDto::ChangedSinceReady,
        }
    }
}

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
    pub status: StatusDto,
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
            status: s.status.into(),
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
    pub is_share_ready: bool,
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
            is_share_ready: v.is_share_ready,
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
    ShareReadyChanged {
        item_id: String,
    },
    Exported {
        item_id: String,
        destination: String,
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
            EngineEvent::ShareReadyChanged { item } => {
                EngineEventDto::ShareReadyChanged { item_id: item.to_string() }
            }
            EngineEvent::Exported { item, destination } => EngineEventDto::Exported {
                item_id: item.to_string(),
                destination: destination.to_string_lossy().into_owned(),
            },
            EngineEvent::ItemMissing { item } => EngineEventDto::ItemMissing { item_id: item.to_string() },
            EngineEvent::ItemUpdated { item } => EngineEventDto::ItemUpdated { item_id: item.to_string() },
            EngineEvent::RootsChanged => EngineEventDto::RootsChanged,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use folyvo_core::engine::EngineEvent;
    use folyvo_core::engine::VersionView;
    use folyvo_core::model::{ContentHash, ItemId, ItemStatus, VersionId, VersionOrigin};
    use serde_json::json;

    #[test]
    fn status_serialises_in_camel_case() {
        assert_eq!(serde_json::to_value(StatusDto::ChangedSinceReady).unwrap(), json!("changedSinceReady"));
        assert_eq!(StatusDto::from(ItemStatus::Ready), StatusDto::Ready);
        assert_eq!(StatusDto::from(ItemStatus::NotShared), StatusDto::NotShared);
    }

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
            is_share_ready: true,
        };
        let json = serde_json::to_value(VersionDto::from(v)).unwrap();
        assert_eq!(json["id"], "ver_0a");
        assert_eq!(json["seq"], 3);
        assert_eq!(json["createdAt"], 1_700_000_000_000i64);
        assert_eq!(json["origin"], "restore");
        assert_eq!(json["note"], "Sent to Acme");
        assert_eq!(json["isShareReady"], true);
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
    }
}
