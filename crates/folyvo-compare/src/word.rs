//! Word documents: paragraphs and tables, in reading order.

use crate::archive::Archive;
use crate::xml::{attr, Ev, Xml};
use crate::{Block, Budget, ParaStyle, Reading, Section, Unit, Unreadable};

pub(crate) fn read_word(archive: &mut Archive, budget: &mut Budget) -> Reading {
    match read_parts(archive, budget) {
        Ok(blocks) => Reading::Text { unit: Unit::Paragraph, blocks },
        Err(why) => Reading::Unreadable(why),
    }
}

fn read_parts(archive: &mut Archive, budget: &mut Budget) -> Result<Vec<Block>, Unreadable> {
    let body = archive.read_part("word/document.xml", budget)?.ok_or(Unreadable::Malformed)?;
    blocks_of(&body, Section::Body, budget)
}

/// A paragraph being read, with any text-box paragraphs found inside it.
struct Para {
    text: String,
    style: ParaStyle,
    /// How many tables were open when it started: equal to the open count means it sits in a cell.
    tables: usize,
    inner: Vec<Block>,
}

/// A table being read.
#[derive(Default)]
struct Table {
    rows: Vec<Vec<String>>,
    row: Vec<String>,
    cell: Option<String>,
    paras: usize,
}

/// The paragraphs and tables of one part, in order. Iterative, so deep nesting cannot overflow.
pub(crate) fn blocks_of(xml: &[u8], section: Section, budget: &mut Budget) -> Result<Vec<Block>, Unreadable> {
    let mut reader = Xml::new(xml);
    let mut out = Vec::new();
    let mut path: Vec<String> = Vec::new();
    let mut paras: Vec<Para> = Vec::new();
    let mut tables: Vec<Table> = Vec::new();
    while let Some(ev) = reader.next(budget)? {
        let (name, attrs, opens) = match &ev {
            Ev::Start(n, a) => (n.as_str(), a.as_slice(), true),
            Ev::Empty(n, a) => (n.as_str(), a.as_slice(), false),
            Ev::End(n) => {
                path.pop();
                match n.as_str() {
                    "p" => finish_para(&mut paras, &mut tables, &mut out, section, budget)?,
                    "tc" => {
                        if let Some(t) = tables.last_mut() {
                            let cell = t.cell.take().unwrap_or_default();
                            t.row.push(cell);
                        }
                    }
                    "tr" => {
                        if let Some(t) = tables.last_mut() {
                            let row = std::mem::take(&mut t.row);
                            t.rows.push(row);
                        }
                    }
                    "tbl" => finish_table(&mut paras, &mut tables, &mut out, section),
                    _ => {}
                }
                continue;
            }
            Ev::Text(text) => {
                if path.last().is_some_and(|p| p == "t") {
                    if let Some(p) = paras.last_mut() {
                        p.text.push_str(text);
                    }
                }
                continue;
            }
        };
        let parent = path.last().map(String::as_str);
        match name {
            "p" => paras.push(Para {
                text: String::new(),
                style: ParaStyle::Body,
                tables: tables.len(),
                inner: Vec::new(),
            }),
            "tab" if parent == Some("r") => push_text(&mut paras, "\t"),
            "br" | "cr" if parent == Some("r") => push_text(&mut paras, "\n"),
            "pStyle" if parent == Some("pPr") => {
                if let (Some(p), Some(style)) = (paras.last_mut(), attr(attrs, "val").and_then(heading)) {
                    p.style = style;
                }
            }
            "numPr" if parent == Some("pPr") => {
                if let Some(p) = paras.last_mut().filter(|p| p.style == ParaStyle::Body) {
                    p.style = ParaStyle::ListItem;
                }
            }
            "tbl" => tables.push(Table { paras: paras.len(), ..Table::default() }),
            "tc" => {
                if let Some(t) = tables.last_mut() {
                    t.cell = Some(String::new());
                }
            }
            _ => {}
        }
        if opens {
            path.push(name.to_string());
        } else if name == "p" {
            finish_para(&mut paras, &mut tables, &mut out, section, budget)?;
        }
    }
    Ok(out)
}

fn push_text(paras: &mut [Para], s: &str) {
    if let Some(p) = paras.last_mut() {
        p.text.push_str(s);
    }
}

/// `Title` → a title, `Heading1`–`Heading9` (any case) → that level.
fn heading(style: &str) -> Option<ParaStyle> {
    let lower = style.to_ascii_lowercase();
    if lower == "title" {
        return Some(ParaStyle::Heading(0));
    }
    let level = lower.strip_prefix("heading")?.trim().parse::<u8>().ok()?;
    (1..=9).contains(&level).then_some(ParaStyle::Heading(level))
}

/// A paragraph ended: into the open cell, into the paragraph that holds its text box, or out.
fn finish_para(
    paras: &mut Vec<Para>,
    tables: &mut [Table],
    out: &mut Vec<Block>,
    section: Section,
    budget: &mut Budget,
) -> Result<(), Unreadable> {
    let Some(p) = paras.pop() else { return Ok(()) };
    budget.add_text(p.text.len())?;
    let in_cell = !tables.is_empty()
        && p.tables == tables.len()
        && tables.last().is_some_and(|t| t.paras == paras.len());
    if in_cell {
        let cell = tables.last_mut().and_then(|t| t.cell.as_mut());
        if let Some(cell) = cell {
            for text in std::iter::once(p.text.as_str()).chain(p.inner.iter().filter_map(block_text)) {
                if text.trim().is_empty() {
                    continue;
                }
                if !cell.is_empty() {
                    cell.push('\n');
                }
                cell.push_str(text);
            }
        }
        return Ok(());
    }
    let mut blocks = Vec::new();
    if !p.text.trim().is_empty() {
        blocks.push(Block::Paragraph { section, style: p.style, text: p.text });
    }
    blocks.extend(p.inner);
    match paras.last_mut() {
        Some(outer) => outer.inner.extend(blocks),
        None => out.extend(blocks),
    }
    Ok(())
}

fn block_text(b: &Block) -> Option<&str> {
    match b {
        Block::Paragraph { text, .. } => Some(text),
        Block::Table { .. } => None,
    }
}

/// A table ended: flattened into the cell of a table around it, or kept as a table.
fn finish_table(paras: &mut [Para], tables: &mut Vec<Table>, out: &mut Vec<Block>, section: Section) {
    let Some(t) = tables.pop() else { return };
    if t.rows.iter().all(|r| r.iter().all(|c| c.trim().is_empty())) {
        return;
    }
    if let Some(cell) = tables.last_mut().and_then(|outer| outer.cell.as_mut()) {
        let text = t.rows.iter().map(|r| r.join("\t")).collect::<Vec<_>>().join("\n");
        if !cell.is_empty() {
            cell.push('\n');
        }
        cell.push_str(&text);
        return;
    }
    let table = Block::Table { section, rows: t.rows };
    match paras.last_mut() {
        Some(outer) => outer.inner.push(table),
        None => out.push(table),
    }
}

#[cfg(test)]
mod tests {
    use crate::testkit::{docx, wp};
    use crate::{read, read_with, Block, Limits, ParaStyle, Reading, Section, Unit, Unreadable};

    fn blocks(body: &str) -> Vec<Block> {
        let d = tempfile::tempdir().unwrap();
        match read(&docx(d.path(), body, &[]), "doc.docx") {
            Reading::Text { unit: Unit::Paragraph, blocks } => blocks,
            other => panic!("{other:?}"),
        }
    }

    fn p(style: ParaStyle, text: &str) -> Block {
        Block::Paragraph { section: Section::Body, style, text: text.into() }
    }

    fn body(text: &str) -> Block {
        p(ParaStyle::Body, text)
    }

    #[test]
    fn paragraphs_keep_their_text_tabs_and_breaks() {
        let got = blocks(
            "<w:p><w:pPr><w:tabs><w:tab w:val=\"left\" w:pos=\"720\"/></w:tabs></w:pPr>\
             <w:r><w:t>Total</w:t><w:tab/><w:t>€10,200</w:t><w:br/><w:t>net</w:t></w:r></w:p>",
        );
        assert_eq!(got, vec![body("Total\t€10,200\nnet")]);
    }

    #[test]
    fn split_runs_join_into_one_paragraph() {
        let got =
            blocks("<w:p><w:r><w:t>Pro</w:t></w:r><w:r><w:t>pos</w:t></w:r><w:r><w:t>al</w:t></w:r></w:p>");
        assert_eq!(got, vec![body("Proposal")]);
    }

    #[test]
    fn headings_and_list_items_are_marked() {
        let styled = |s: &str, t: &str| {
            format!("<w:p><w:pPr><w:pStyle w:val=\"{s}\"/></w:pPr><w:r><w:t>{t}</w:t></w:r></w:p>")
        };
        let got = blocks(&format!(
            "{}{}<w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>item</w:t></w:r></w:p>",
            styled("Title", "Proposal"),
            styled("Heading2", "Costs"),
        ));
        assert_eq!(
            got,
            vec![
                p(ParaStyle::Heading(0), "Proposal"),
                p(ParaStyle::Heading(2), "Costs"),
                p(ParaStyle::ListItem, "item")
            ]
        );
    }

    #[test]
    fn tracked_changes_read_as_accepted() {
        let got = blocks(
            "<w:p><w:ins w:id=\"1\"><w:r><w:t>new</w:t></w:r></w:ins>\
             <w:del w:id=\"2\"><w:r><w:delText>old</w:delText></w:r></w:del></w:p>",
        );
        assert_eq!(got, vec![body("new")]);
    }

    #[test]
    fn tables_become_rows_of_cells() {
        let cell = |t: &str| format!("<w:tc>{}</w:tc>", wp(t));
        let got = blocks(&format!(
            "<w:tbl><w:tr>{}{}</w:tr><w:tr>{}{}</w:tr></w:tbl>",
            cell("Setup"),
            cell("€4,800"),
            cell("Support"),
            cell("€1,800")
        ));
        assert_eq!(
            got,
            vec![Block::Table {
                section: Section::Body,
                rows: vec![vec!["Setup".into(), "€4,800".into()], vec!["Support".into(), "€1,800".into()]],
            }]
        );
    }

    #[test]
    fn text_boxes_follow_their_paragraph() {
        let got = blocks(&format!(
            "<w:p><w:r><w:t>A</w:t></w:r><w:r><w:pict><v:shape><v:textbox><w:txbxContent>{}</w:txbxContent></v:textbox></v:shape></w:pict></w:r></w:p>{}",
            wp("B"),
            wp("C")
        ));
        assert_eq!(got, vec![body("A"), body("B"), body("C")]);
    }

    #[test]
    fn empty_paragraphs_are_dropped() {
        assert_eq!(blocks(&format!("<w:p/><w:p><w:r><w:t></w:t></w:r></w:p>{}", wp("x"))), vec![body("x")]);
    }

    #[test]
    fn a_broken_document_is_malformed() {
        let d = tempfile::tempdir().unwrap();
        let path = crate::testkit::zip_file(
            d.path(),
            "a.docx",
            &[("word/document.xml", b"<w:document><w:body><w:p>")],
        );
        assert_eq!(read(&path, "a.docx"), Reading::Unreadable(Unreadable::Malformed));
    }

    #[test]
    fn a_slow_reading_stops() {
        let d = tempfile::tempdir().unwrap();
        let body: String = (0..5000).map(|i| wp(&i.to_string())).collect();
        let limits = Limits { time: std::time::Duration::ZERO, ..Limits::DEFAULT };
        assert_eq!(
            read_with(&docx(d.path(), &body, &[]), "a.docx", &limits),
            Reading::Unreadable(Unreadable::TooSlow)
        );
    }
}
