use super::ids::{ContentHash, ItemId, VersionId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionOrigin {
    Initial,
    Auto,
    Restore,
}

impl VersionOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            VersionOrigin::Initial => "initial",
            VersionOrigin::Auto => "auto",
            VersionOrigin::Restore => "restore",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "initial" => Some(VersionOrigin::Initial),
            "auto" => Some(VersionOrigin::Auto),
            "restore" => Some(VersionOrigin::Restore),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub id: VersionId,
    pub item_id: ItemId,
    pub seq: i64,
    pub content_hash: ContentHash,
    pub size: u64,
    pub created_at: i64,
    pub origin: VersionOrigin,
    pub restored_from: Option<VersionId>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareEventKind {
    Exported,
    /// The file went to another app through the share sheet (Mail, AirDrop, Copy, ...).
    Handed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareEvent {
    pub item_id: ItemId,
    pub version_id: VersionId,
    pub kind: ShareEventKind,
    /// For `Handed`: `mail`, `airdrop`, `messages`, `copy` or `other:<title>`.
    pub service: Option<String>,
    pub target_path: Option<String>,
    pub at: i64,
}

impl ShareEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ShareEventKind::Exported => "exported",
            ShareEventKind::Handed => "handed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "exported" => Some(ShareEventKind::Exported),
            "handed" => Some(ShareEventKind::Handed),
            _ => None,
        }
    }
}
