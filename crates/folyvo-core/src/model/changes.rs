//! What changed in a save since the save before it, as counts. Never the document's text.

use super::ids::VersionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeOutcome {
    /// The first save of a file: nothing to compare with.
    First,
    /// Text Folyvo reads changed; the counts say what.
    Text,
    /// The text Folyvo reads is the same; something else in the file changed.
    SameText,
    /// The save before has the very same content (a restore of the newest save, say).
    Identical,
    /// Folyvo could not read one of the two, or they are different kinds of file.
    Unknown,
}

impl ChangeOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeOutcome::First => "first",
            ChangeOutcome::Text => "text",
            ChangeOutcome::SameText => "same_text",
            ChangeOutcome::Identical => "identical",
            ChangeOutcome::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::First, Self::Text, Self::SameText, Self::Identical, Self::Unknown]
            .into_iter()
            .find(|o| o.as_str() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeUnit {
    Line,
    Paragraph,
    Cell,
    Sheet,
    Page,
}

impl ChangeUnit {
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeUnit::Line => "line",
            ChangeUnit::Paragraph => "paragraph",
            ChangeUnit::Cell => "cell",
            ChangeUnit::Sheet => "sheet",
            ChangeUnit::Page => "page",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Line, Self::Paragraph, Self::Cell, Self::Sheet, Self::Page]
            .into_iter()
            .find(|u| u.as_str() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangeCount {
    pub unit: ChangeUnit,
    pub added: u32,
    pub removed: u32,
    pub changed: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeRecord {
    /// The save this one was compared with; `None` for the first save.
    pub compared_to: Option<VersionId>,
    pub outcome: ChangeOutcome,
    /// For `Text` at least one; empty otherwise. In unit order.
    pub counts: Vec<ChangeCount>,
    /// The version of the reader that made it; a newer reader makes it again.
    pub reader: i64,
}
