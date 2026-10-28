//! What changed between two readings: counts for a summary, and the later reading marked.

use std::time::{Duration, Instant};

use similar::{capture_diff_slices_deadline, Algorithm, ChangeTag, DiffOp, TextDiff};

use crate::{Block, ParaStyle, Reading, Section, Unit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Something Folyvo reads changed; `counts` says what.
    Text,
    /// The text Folyvo reads is the same; something else in the file changed.
    SameText,
    /// One side could not be read, or the two are different kinds of file.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Count {
    pub unit: Unit,
    pub added: u32,
    pub removed: u32,
    pub changed: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Same,
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Span {
    Same(String),
    Added(String),
    Removed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkedCell {
    pub mark: Mark,
    pub spans: Vec<Span>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkedBlock {
    Paragraph { section: Section, style: ParaStyle, mark: Mark, spans: Vec<Span> },
    Table { section: Section, mark: Mark, rows: Vec<Vec<MarkedCell>> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Marked {
    Text(Vec<MarkedBlock>),
    Nothing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changes {
    pub outcome: Outcome,
    /// Sorted by unit; units with nothing to count are left out.
    pub counts: Vec<Count>,
    pub marked: Marked,
}

/// How long one comparison may search for the best match before it settles for a good one.
const DEADLINE: Duration = Duration::from_secs(2);
/// Two paragraphs this alike (by words) are one changed paragraph, not one removed and one added.
const ALIKE: f32 = 0.5;
/// The same by letters, set higher because unrelated sentences still share many letters.
const ALIKE_LETTERS: f32 = 0.6;
/// Only text this short (in bytes) is compared letter by letter.
const SHORT: usize = 200;
/// How far ahead a removed block looks for an added one it is like.
const LOOK_AHEAD: usize = 20;

pub fn compare(before: &Reading, after: &Reading) -> Changes {
    match (before, after) {
        (Reading::Text { unit: a, blocks: old }, Reading::Text { unit: b, blocks: new }) if a == b => {
            compare_text(*a, old, new)
        }
        _ => Changes { outcome: Outcome::Unknown, counts: Vec::new(), marked: Marked::Nothing },
    }
}

/// Counts per unit, kept in unit order.
#[derive(Default)]
pub(crate) struct Tally(Vec<Count>);

impl Tally {
    fn entry(&mut self, unit: Unit) -> &mut Count {
        let at = match self.0.iter().position(|c| c.unit == unit) {
            Some(at) => at,
            None => {
                self.0.push(Count { unit, added: 0, removed: 0, changed: 0 });
                self.0.len() - 1
            }
        };
        &mut self.0[at]
    }

    pub(crate) fn add(&mut self, unit: Unit, n: u32) {
        self.entry(unit).added += n;
    }

    pub(crate) fn remove(&mut self, unit: Unit, n: u32) {
        self.entry(unit).removed += n;
    }

    pub(crate) fn change(&mut self, unit: Unit, n: u32) {
        self.entry(unit).changed += n;
    }

    pub(crate) fn finish(mut self) -> Vec<Count> {
        self.0.retain(|c| c.added + c.removed + c.changed > 0);
        self.0.sort_by_key(|c| c.unit);
        self.0
    }
}

/// What two blocks are matched on: where they are, what they are, and their words.
#[derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Key(u8, bool, String);

fn key(b: &Block) -> Key {
    let squash = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    match b {
        Block::Paragraph { section, text, .. } => Key(*section as u8, false, squash(text)),
        Block::Table { section, rows, .. } => Key(
            *section as u8,
            true,
            rows.iter()
                .map(|r| r.iter().map(|c| squash(c)).collect::<Vec<_>>().join("\u{1f}"))
                .collect::<Vec<_>>()
                .join("\u{1e}"),
        ),
    }
}

fn compare_text(unit: Unit, old: &[Block], new: &[Block]) -> Changes {
    let (old_keys, new_keys): (Vec<Key>, Vec<Key>) =
        (old.iter().map(key).collect(), new.iter().map(key).collect());
    let ops = capture_diff_slices_deadline(
        Algorithm::Patience,
        &old_keys,
        &new_keys,
        Some(Instant::now() + DEADLINE),
    );
    let mut tally = Tally::default();
    let mut marked = Vec::new();
    for op in ops {
        let (_, o, n) = op.as_tag_tuple();
        match op {
            DiffOp::Equal { .. } => marked.extend(new[n].iter().map(|b| same(b, Mark::Same))),
            _ => pair_up(unit, &old[o], &new[n], &mut tally, &mut marked),
        }
    }
    let counts = tally.finish();
    let outcome = if counts.is_empty() { Outcome::SameText } else { Outcome::Text };
    Changes { outcome, counts, marked: Marked::Text(marked) }
}

/// A run of removed and added blocks: alike pairs become changed blocks, the rest stay removed
/// or added, in order.
fn pair_up(unit: Unit, old: &[Block], new: &[Block], tally: &mut Tally, out: &mut Vec<MarkedBlock>) {
    let (mut i, mut j) = (0, 0);
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && alike(&old[i], &new[j]) {
            out.push(changed(unit, &old[i], &new[j], tally));
            i += 1;
            j += 1;
        } else if i < old.len() && new[j..].iter().take(LOOK_AHEAD).any(|b| alike(&old[i], b)) {
            out.push(added(unit, &new[j], tally));
            j += 1;
        } else if i < old.len() {
            out.push(removed(unit, &old[i], tally));
            i += 1;
        } else {
            out.push(added(unit, &new[j], tally));
            j += 1;
        }
    }
}

fn alike(a: &Block, b: &Block) -> bool {
    match (a, b) {
        (Block::Paragraph { section: s1, text: t1, .. }, Block::Paragraph { section: s2, text: t2, .. }) => {
            s1 == s2 && similar_text(t1, t2)
        }
        (Block::Table { section: s1, rows: r1 }, Block::Table { section: s2, rows: r2 }) => {
            s1 == s2 && r1.len() == r2.len() && r1.iter().zip(r2).all(|(a, b)| a.len() == b.len())
        }
        _ => false,
    }
}

/// Alike by words, or, for short runs of text without spaces such as `y,2` and `y,3`, by letters.
fn similar_text(a: &str, b: &str) -> bool {
    let config = || {
        let mut c = TextDiff::configure();
        c.algorithm(Algorithm::Patience).timeout(DEADLINE);
        c
    };
    config().diff_words(a, b).ratio() >= ALIKE
        || (a.len().max(b.len()) <= SHORT && config().diff_chars(a, b).ratio() >= ALIKE_LETTERS)
}

fn cells(rows: &[Vec<String>]) -> u32 {
    rows.iter().map(|r| r.len() as u32).sum()
}

fn added(unit: Unit, b: &Block, tally: &mut Tally) -> MarkedBlock {
    match b {
        Block::Paragraph { .. } => tally.add(unit, 1),
        Block::Table { rows, .. } => tally.add(Unit::Cell, cells(rows)),
    }
    same(b, Mark::Added)
}

fn removed(unit: Unit, b: &Block, tally: &mut Tally) -> MarkedBlock {
    match b {
        Block::Paragraph { .. } => tally.remove(unit, 1),
        Block::Table { rows, .. } => tally.remove(Unit::Cell, cells(rows)),
    }
    same(b, Mark::Removed)
}

/// `b` with every part carrying `mark`.
fn same(b: &Block, mark: Mark) -> MarkedBlock {
    let span = |t: &str| match mark {
        Mark::Added => Span::Added(t.to_string()),
        Mark::Removed => Span::Removed(t.to_string()),
        Mark::Same | Mark::Changed => Span::Same(t.to_string()),
    };
    match b {
        Block::Paragraph { section, style, text } => {
            MarkedBlock::Paragraph { section: *section, style: *style, mark, spans: vec![span(text)] }
        }
        Block::Table { section, rows } => MarkedBlock::Table {
            section: *section,
            mark,
            rows: rows
                .iter()
                .map(|r| r.iter().map(|c| MarkedCell { mark, spans: vec![span(c)] }).collect())
                .collect(),
        },
    }
}

/// Two alike blocks: a paragraph marked word by word, or a table cell by cell.
fn changed(unit: Unit, old: &Block, new: &Block, tally: &mut Tally) -> MarkedBlock {
    match (old, new) {
        (Block::Paragraph { text: a, .. }, Block::Paragraph { section, style, text: b }) => {
            tally.change(unit, 1);
            MarkedBlock::Paragraph {
                section: *section,
                style: *style,
                mark: Mark::Changed,
                spans: words(a, b),
            }
        }
        (Block::Table { rows: a, .. }, Block::Table { section, rows: b }) => {
            let rows: Vec<Vec<MarkedCell>> = a
                .iter()
                .zip(b)
                .map(|(ra, rb)| {
                    ra.iter()
                        .zip(rb)
                        .map(|(ca, cb)| {
                            if ca == cb {
                                MarkedCell { mark: Mark::Same, spans: vec![Span::Same(cb.clone())] }
                            } else {
                                tally.change(Unit::Cell, 1);
                                MarkedCell { mark: Mark::Changed, spans: words(ca, cb) }
                            }
                        })
                        .collect()
                })
                .collect();
            let mark = if rows.iter().flatten().any(|c| c.mark == Mark::Changed) {
                Mark::Changed
            } else {
                Mark::Same
            };
            MarkedBlock::Table { section: *section, mark, rows }
        }
        _ => unreachable!("only alike blocks are paired"),
    }
}

/// `after`'s words, with those not in `before` added and those gone shown removed.
pub(crate) fn words(before: &str, after: &str) -> Vec<Span> {
    let diff =
        TextDiff::configure().algorithm(Algorithm::Patience).timeout(DEADLINE).diff_words(before, after);
    let mut spans: Vec<Span> = Vec::new();
    for change in diff.iter_all_changes() {
        let text = change.value();
        match (change.tag(), spans.last_mut()) {
            (ChangeTag::Equal, Some(Span::Same(s)))
            | (ChangeTag::Insert, Some(Span::Added(s)))
            | (ChangeTag::Delete, Some(Span::Removed(s))) => s.push_str(text),
            (ChangeTag::Equal, _) => spans.push(Span::Same(text.to_string())),
            (ChangeTag::Insert, _) => spans.push(Span::Added(text.to_string())),
            (ChangeTag::Delete, _) => spans.push(Span::Removed(text.to_string())),
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{docx, wp};
    use crate::{read, Block, ParaStyle, Reading, Section, Unit, Unreadable};

    fn text(unit: Unit, paras: &[&str]) -> Reading {
        Reading::Text { unit, blocks: paras.iter().map(|t| para(Section::Body, t)).collect() }
    }

    fn para(section: Section, t: &str) -> Block {
        Block::Paragraph { section, style: ParaStyle::Body, text: t.to_string() }
    }

    fn table(rows: &[&[&str]]) -> Block {
        Block::Table {
            section: Section::Body,
            rows: rows.iter().map(|r| r.iter().map(|c| c.to_string()).collect()).collect(),
        }
    }

    fn count(unit: Unit, added: u32, removed: u32, changed: u32) -> Count {
        Count { unit, added, removed, changed }
    }

    fn marks(c: &Changes) -> Vec<Mark> {
        match &c.marked {
            Marked::Text(blocks) => blocks
                .iter()
                .map(|b| match b {
                    MarkedBlock::Paragraph { mark, .. } | MarkedBlock::Table { mark, .. } => *mark,
                })
                .collect(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_added_paragraph_counts_one_added() {
        let c = compare(&text(Unit::Paragraph, &["a", "b"]), &text(Unit::Paragraph, &["a", "x", "b"]));
        assert_eq!(c.outcome, Outcome::Text);
        assert_eq!(c.counts, vec![count(Unit::Paragraph, 1, 0, 0)]);
        assert_eq!(marks(&c), vec![Mark::Same, Mark::Added, Mark::Same]);
    }

    #[test]
    fn a_reworded_paragraph_is_changed_with_word_spans() {
        let c = compare(
            &text(Unit::Paragraph, &["Support, 12 months"]),
            &text(Unit::Paragraph, &["Support, 18 months"]),
        );
        assert_eq!(c.counts, vec![count(Unit::Paragraph, 0, 0, 1)]);
        let Marked::Text(blocks) = &c.marked else { panic!() };
        let MarkedBlock::Paragraph { mark: Mark::Changed, spans, .. } = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!(
            spans,
            &vec![
                Span::Same("Support, ".into()),
                Span::Removed("12".into()),
                Span::Added("18".into()),
                Span::Same(" months".into())
            ]
        );
    }

    #[test]
    fn an_unrelated_replacement_is_removed_and_added() {
        let c = compare(
            &text(Unit::Paragraph, &["Kind regards"]),
            &text(Unit::Paragraph, &["Total due in 30 days"]),
        );
        assert_eq!(c.counts, vec![count(Unit::Paragraph, 1, 1, 0)]);
        assert_eq!(marks(&c), vec![Mark::Removed, Mark::Added]);
    }

    #[test]
    fn a_changed_table_cell_counts_cells() {
        let before = table(&[&["Setup", "€4,800"], &["Support", "€1,800"], &["Total", "€10,200"]]);
        let after = table(&[&["Setup", "€4,800"], &["Support", "€2,400"], &["Total", "€10,800"]]);
        let c = compare(
            &Reading::Text { unit: Unit::Paragraph, blocks: vec![para(Section::Body, "Costs"), before] },
            &Reading::Text { unit: Unit::Paragraph, blocks: vec![para(Section::Body, "Costs"), after] },
        );
        assert_eq!(c.counts, vec![count(Unit::Cell, 0, 0, 2)]);
        let Marked::Text(blocks) = &c.marked else { panic!() };
        let MarkedBlock::Table { mark: Mark::Changed, rows, .. } = &blocks[1] else { panic!("{blocks:?}") };
        assert_eq!(rows[1][1].mark, Mark::Changed);
        assert_eq!(rows[0][0].mark, Mark::Same);
    }

    #[test]
    fn a_reshaped_table_is_removed_and_added() {
        let c = compare(
            &Reading::Text { unit: Unit::Paragraph, blocks: vec![table(&[&["a", "b"], &["c", "d"]])] },
            &Reading::Text {
                unit: Unit::Paragraph,
                blocks: vec![table(&[&["a", "b"], &["c", "d"], &["e", "f"]])],
            },
        );
        assert_eq!(c.counts, vec![count(Unit::Cell, 6, 4, 0)]);
    }

    #[test]
    fn lines_count_as_lines() {
        let c = compare(&text(Unit::Line, &["x,1", "y,2"]), &text(Unit::Line, &["x,1", "y,3"]));
        assert_eq!(c.counts, vec![count(Unit::Line, 0, 0, 1)]);
    }

    #[test]
    fn equal_readings_are_same_text() {
        let c = compare(&text(Unit::Paragraph, &["a"]), &text(Unit::Paragraph, &["a"]));
        assert_eq!((c.outcome, c.counts.len()), (Outcome::SameText, 0));
        assert_eq!(marks(&c), vec![Mark::Same]);
    }

    #[test]
    fn unreadable_or_mismatched_is_unknown() {
        let c = compare(&Reading::Unreadable(Unreadable::TooLarge), &text(Unit::Paragraph, &["a"]));
        assert_eq!((c.outcome, c.marked), (Outcome::Unknown, Marked::Nothing));
        assert_eq!(
            compare(&text(Unit::Line, &["a"]), &text(Unit::Paragraph, &["a"])).outcome,
            Outcome::Unknown
        );
    }

    #[test]
    fn sections_do_not_match_across() {
        let c = compare(
            &Reading::Text { unit: Unit::Paragraph, blocks: vec![para(Section::Body, "Page 1")] },
            &Reading::Text { unit: Unit::Paragraph, blocks: vec![para(Section::Footer, "Page 1")] },
        );
        assert_eq!(c.counts, vec![count(Unit::Paragraph, 1, 1, 0)]);
    }

    #[test]
    fn a_footer_change_is_a_text_change() {
        let part = |inner: &str| {
            format!(
                r#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">{inner}</w:ftr>"#
            )
        };
        let d1 = tempfile::tempdir().unwrap();
        let d2 = tempfile::tempdir().unwrap();
        let before = docx(d1.path(), &wp("B"), &[("word/footer1.xml", &part(&wp("Page 1 of 2")))]);
        let after = docx(d2.path(), &wp("B"), &[("word/footer1.xml", &part(&wp("Page 1 of 3")))]);
        let c = compare(&read(&before, "a.docx"), &read(&after, "a.docx"));
        assert_eq!(c.outcome, Outcome::Text);
        assert_eq!(c.counts, vec![count(Unit::Paragraph, 0, 0, 1)]);
    }
}
