//! Plain text: paragraphs for prose kinds, lines for the rest.

use crate::{Block, Budget, ParaStyle, Reading, Section, Unit, Unreadable};

/// `bytes` as UTF-8 text split by `unit`: lines, or paragraphs separated by blank lines. Empty
/// lines and paragraphs are left out.
pub(crate) fn read_text(bytes: &[u8], unit: Unit, budget: &mut Budget) -> Reading {
    match split(bytes, unit, budget) {
        Ok(blocks) => Reading::Text { unit, blocks },
        Err(why) => Reading::Unreadable(why),
    }
}

fn split(bytes: &[u8], unit: Unit, budget: &mut Budget) -> Result<Vec<Block>, Unreadable> {
    let text = std::str::from_utf8(bytes).map_err(|_| Unreadable::Malformed)?;
    if text.contains('\0') {
        return Err(Unreadable::Malformed);
    }
    budget.add_text(text.len())?;
    let lines = text.lines(); // `lines` drops a trailing `\r` too
    let para = |text: String| Block::Paragraph { section: Section::Body, style: ParaStyle::Body, text };
    let mut blocks = Vec::new();
    if unit == Unit::Line {
        for line in lines {
            budget.tick()?;
            if !line.trim().is_empty() {
                blocks.push(para(line.to_string()));
            }
        }
        return Ok(blocks);
    }
    let mut current: Vec<&str> = Vec::new();
    for line in lines.chain(std::iter::once("")) {
        budget.tick()?;
        if line.trim().is_empty() {
            if !current.is_empty() {
                blocks.push(para(current.join("\n")));
                current.clear();
            }
        } else {
            current.push(line);
        }
    }
    Ok(blocks)
}

#[cfg(test)]
mod tests {
    use crate::*;
    use std::path::PathBuf;

    fn file(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.path().join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn p(text: &str) -> Block {
        Block::Paragraph { section: Section::Body, style: ParaStyle::Body, text: text.into() }
    }

    fn read_named(name: &str, bytes: &[u8]) -> Reading {
        let d = tempfile::tempdir().unwrap();
        read(&file(&d, name, bytes), name)
    }

    #[test]
    fn paragraph_kinds_split_at_blank_lines() {
        assert_eq!(
            read_named("n.txt", b"a\nb\n\n\n\nc\n"),
            Reading::Text { unit: Unit::Paragraph, blocks: vec![p("a\nb"), p("c")] }
        );
    }

    #[test]
    fn line_kinds_split_into_lines() {
        assert_eq!(
            read_named("t.csv", b"x,1\ny,2\n"),
            Reading::Text { unit: Unit::Line, blocks: vec![p("x,1"), p("y,2")] }
        );
    }

    #[test]
    fn crlf_is_one_line_break() {
        assert_eq!(
            read_named("a.log", b"a\r\nb"),
            Reading::Text { unit: Unit::Line, blocks: vec![p("a"), p("b")] }
        );
    }

    #[test]
    fn not_utf8_is_unreadable() {
        assert_eq!(read_named("a.txt", &[0xff, 0xfe, 0x00]), Reading::Unreadable(Unreadable::Malformed));
    }

    #[test]
    fn a_nul_byte_is_unreadable() {
        assert_eq!(read_named("a.txt", b"a\0b"), Reading::Unreadable(Unreadable::Malformed));
    }

    #[test]
    fn a_file_over_the_limit_is_too_large() {
        let d = tempfile::tempdir().unwrap();
        let limits = Limits { file_bytes: 4, ..Limits::DEFAULT };
        assert_eq!(
            read_with(&file(&d, "a.txt", b"12345"), "a.txt", &limits),
            Reading::Unreadable(Unreadable::TooLarge)
        );
    }

    #[test]
    fn text_over_the_text_limit_is_too_large() {
        let d = tempfile::tempdir().unwrap();
        let limits = Limits { text_bytes: 3, ..Limits::DEFAULT };
        assert_eq!(
            read_with(&file(&d, "a.txt", b"abcd"), "a.txt", &limits),
            Reading::Unreadable(Unreadable::TooLarge)
        );
    }

    #[test]
    fn an_unknown_kind_is_not_supported() {
        assert_eq!(read_named("x.key", b"whatever"), Reading::Unreadable(Unreadable::NotSupported));
    }

    #[test]
    fn cell_refs_read_and_print_as_a1() {
        assert_eq!(CellRef::parse("B7"), Some(CellRef { row: 7, col: 2 }));
        assert_eq!(CellRef::parse("AA10"), Some(CellRef { row: 10, col: 27 }));
        for bad in ["7B", "", "A0", "A", "b7x"] {
            assert_eq!(CellRef::parse(bad), None, "{bad}");
        }
        assert_eq!(CellRef { row: 1, col: 28 }.to_string(), "AB1");
    }
}
