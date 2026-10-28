//! What a file says, as Folyvo reads it.

use std::collections::BTreeMap;
use std::fmt;

/// What a count is counting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Unit {
    Line,
    Paragraph,
    Cell,
    Sheet,
    Page,
}

impl Unit {
    pub fn as_str(self) -> &'static str {
        match self {
            Unit::Line => "line",
            Unit::Paragraph => "paragraph",
            Unit::Cell => "cell",
            Unit::Sheet => "sheet",
            Unit::Page => "page",
        }
    }
}

/// Where in a document a block sits. Text is only ever matched within the same section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Body,
    Header,
    Footer,
    Notes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParaStyle {
    Body,
    /// `0` is a title, `1`–`9` the heading levels.
    Heading(u8),
    ListItem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph { section: Section, style: ParaStyle, text: String },
    Table { section: Section, rows: Vec<Vec<String>> },
}

/// A spreadsheet cell's place, counted from 1 (`B7` is row 7, column 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellRef {
    pub row: u32,
    pub col: u32,
}

impl CellRef {
    /// `"B7"` → row 7, column 2. Anything that is not letters then a row number from 1 is `None`.
    pub fn parse(a1: &str) -> Option<CellRef> {
        let split = a1.find(|c: char| c.is_ascii_digit())?;
        let (letters, digits) = a1.split_at(split);
        if letters.is_empty() || letters.len() > 3 || !letters.chars().all(|c| c.is_ascii_uppercase()) {
            return None;
        }
        if !digits.chars().all(|c| c.is_ascii_digit()) || digits.starts_with('0') {
            return None;
        }
        let col = letters.bytes().fold(0u32, |n, b| n * 26 + u32::from(b - b'A' + 1));
        Some(CellRef { row: digits.parse().ok()?, col })
    }
}

impl fmt::Display for CellRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut letters = Vec::new();
        let mut n = self.col;
        while n > 0 {
            let rem = (n - 1) % 26;
            letters.push(b'A' + rem as u8);
            n = (n - 1) / 26;
        }
        letters.reverse();
        write!(f, "{}{}", String::from_utf8_lossy(&letters), self.row)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sheet {
    pub name: String,
    pub cells: BTreeMap<CellRef, String>,
}

/// Why a file could not be read. None of these is shown as an error: the summary just says "Changed".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreadable {
    NotSupported,
    TooLarge,
    TooSlow,
    Malformed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reading {
    /// Prose or lines; `unit` is `Line` or `Paragraph`.
    Text {
        unit: Unit,
        blocks: Vec<Block>,
    },
    Sheets(Vec<Sheet>),
    /// A PDF's words, one string per page.
    Pages(Vec<String>),
    Unreadable(Unreadable),
}
