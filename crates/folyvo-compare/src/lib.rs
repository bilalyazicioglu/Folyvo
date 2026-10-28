//! Reading a document as text, and saying what changed between two readings. Pure Rust, no
//! platform code, no knowledge of the vault: it is handed two files and gives back what it found.

mod archive;
mod budget;
mod reading;
mod sniff;
#[cfg(test)]
mod testkit;
mod text;
mod word;
mod xml;

use std::path::Path;

pub(crate) use budget::Budget;
pub use budget::Limits;
pub use reading::{Block, CellRef, ParaStyle, Reading, Section, Sheet, Unit, Unreadable};
use sniff::Kind;

/// Bumped whenever reading or comparing changes what it finds, so stored summaries are redone.
pub const READER_VERSION: i64 = 1;

/// Kinds read as plain text (UTF-8 only).
pub const TEXT_KINDS: &[&str] =
    &["txt", "text", "md", "markdown", "csv", "tsv", "json", "log", "xml", "yml", "yaml"];
/// The text kinds that are prose, read by paragraph; the rest are read line by line.
pub const PARAGRAPH_KINDS: &[&str] = &["txt", "text", "md", "markdown"];

/// What `path` says. `name_hint` is the document's name, used only for plain text.
pub fn read(path: &Path, name_hint: &str) -> Reading {
    read_with(path, name_hint, &Limits::DEFAULT)
}

pub fn read_with(path: &Path, name_hint: &str, limits: &Limits) -> Reading {
    match std::fs::metadata(path) {
        Ok(m) if m.len() > limits.file_bytes => return Reading::Unreadable(Unreadable::TooLarge),
        Ok(_) => {}
        Err(_) => return Reading::Unreadable(Unreadable::Malformed),
    }
    let mut budget = Budget::new(limits);
    match sniff::sniff(path, name_hint) {
        Kind::Text(unit) => match std::fs::read(path) {
            Ok(bytes) => text::read_text(&bytes, unit, &mut budget),
            Err(_) => Reading::Unreadable(Unreadable::Malformed),
        },
        Kind::Word => match archive::Archive::open(path, limits) {
            Ok(mut zip) => word::read_word(&mut zip, &mut budget),
            Err(why) => Reading::Unreadable(why),
        },
        Kind::Excel | Kind::Pdf | Kind::Other => Reading::Unreadable(Unreadable::NotSupported),
    }
}

/// `Paragraph` or `Line` for a text kind, by the name's extension.
pub(crate) fn text_unit(name: &str) -> Option<Unit> {
    let ext = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
    if PARAGRAPH_KINDS.contains(&ext.as_str()) {
        Some(Unit::Paragraph)
    } else if TEXT_KINDS.contains(&ext.as_str()) {
        Some(Unit::Line)
    } else {
        None
    }
}
