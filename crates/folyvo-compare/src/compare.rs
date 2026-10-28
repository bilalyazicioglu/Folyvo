//! What changed between two readings: counts for a summary, and the later reading marked.

use std::time::{Duration, Instant};

use similar::{capture_diff_slices_deadline, Algorithm, ChangeTag, DiffOp, TextDiff};

use crate::{Block, CellRef, ParaStyle, Reading, Section, Sheet, Unit};

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
    Sheets(Vec<SheetChanges>),
    Pages(Vec<PageChanges>),
    Nothing,
}

/// One page of a PDF, numbered from 1, with its words marked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageChanges {
    pub number: u32,
    pub mark: Mark,
    pub spans: Vec<Span>,
}

/// A sheet, matched by name. An added or removed sheet lists no cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetChanges {
    pub name: String,
    pub mark: Mark,
    pub cells: Vec<CellChange>,
}

/// One cell that differs: both values for a changed cell, one for an added or removed one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellChange {
    pub at: CellRef,
    pub before: Option<String>,
    pub after: Option<String>,
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
/// Text longer than this (in bytes) is not measured for likeness: comparing it word by word costs
/// too much.
const LONG: usize = 16 * 1024;

pub fn compare(before: &Reading, after: &Reading) -> Changes {
    compare_until(before, after, Instant::now() + DEADLINE)
}

/// `compare` with one deadline for all of it: once it passes, nothing more is paired up, so
/// what is left counts as removed and added.
pub(crate) fn compare_until(before: &Reading, after: &Reading, deadline: Instant) -> Changes {
    match (before, after) {
        (Reading::Text { unit: a, blocks: old }, Reading::Text { unit: b, blocks: new }) if a == b => {
            compare_text(*a, old, new, deadline)
        }
        (Reading::Sheets(old), Reading::Sheets(new)) => compare_sheets(old, new),
        (Reading::Pages(old), Reading::Pages(new)) => compare_pages(old, new, deadline),
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

fn compare_text(unit: Unit, old: &[Block], new: &[Block], deadline: Instant) -> Changes {
    let (old_keys, new_keys): (Vec<Key>, Vec<Key>) =
        (old.iter().map(key).collect(), new.iter().map(key).collect());
    let ops = capture_diff_slices_deadline(Algorithm::Patience, &old_keys, &new_keys, Some(deadline));
    let mut tally = Tally::default();
    let mut marked = Vec::new();
    for op in ops {
        let (_, o, n) = op.as_tag_tuple();
        match op {
            DiffOp::Equal { .. } => marked.extend(new[n].iter().map(|b| same(b, Mark::Same))),
            _ => pair_up(unit, &old[o], &new[n], deadline, &mut tally, &mut marked),
        }
    }
    // Past the deadline, matching settled for less than the best, so the counts may not be true.
    // The page can still be shown; the summary only says that something changed.
    if Instant::now() >= deadline {
        return Changes { outcome: Outcome::Unknown, counts: Vec::new(), marked: Marked::Text(marked) };
    }
    let counts = tally.finish();
    let outcome = if counts.is_empty() { Outcome::SameText } else { Outcome::Text };
    Changes { outcome, counts, marked: Marked::Text(marked) }
}

/// Pages by number: a page in both whose words differ is changed, extra pages are added or removed.
fn compare_pages(old: &[String], new: &[String], deadline: Instant) -> Changes {
    let mut tally = Tally::default();
    let mut marked = Vec::new();
    for at in 0..old.len().max(new.len()) {
        let number = at as u32 + 1;
        let (mark, spans) = match (old.get(at), new.get(at)) {
            (Some(a), Some(b)) if a == b => (Mark::Same, vec![Span::Same(b.clone())]),
            (Some(a), Some(b)) => {
                tally.change(Unit::Page, 1);
                (Mark::Changed, words(a, b, deadline))
            }
            (None, Some(b)) => {
                tally.add(Unit::Page, 1);
                (Mark::Added, vec![Span::Added(b.clone())])
            }
            (Some(a), None) => {
                tally.remove(Unit::Page, 1);
                (Mark::Removed, vec![Span::Removed(a.clone())])
            }
            (None, None) => continue,
        };
        marked.push(PageChanges { number, mark, spans });
    }
    let counts = tally.finish();
    let outcome = if counts.is_empty() { Outcome::SameText } else { Outcome::Text };
    Changes { outcome, counts, marked: Marked::Pages(marked) }
}

/// Sheets by name, in the later file's order, with removed sheets after them.
fn compare_sheets(old: &[Sheet], new: &[Sheet]) -> Changes {
    let mut tally = Tally::default();
    let mut marked = Vec::new();
    let named = |sheets: &[Sheet], name: &str| sheets.iter().position(|s| s.name == name);
    for sheet in new {
        let Some(at) = named(old, &sheet.name) else {
            tally.add(Unit::Sheet, 1);
            marked.push(SheetChanges { name: sheet.name.clone(), mark: Mark::Added, cells: Vec::new() });
            continue;
        };
        let (before, after) = (&old[at].cells, &sheet.cells);
        let mut places: Vec<&CellRef> = before.keys().chain(after.keys()).collect();
        places.sort();
        places.dedup();
        let mut cells = Vec::new();
        for place in places {
            let (b, a) = (before.get(place), after.get(place));
            match (b, a) {
                (Some(b), Some(a)) if b == a => continue,
                (Some(_), Some(_)) => tally.change(Unit::Cell, 1),
                (Some(_), None) => tally.remove(Unit::Cell, 1),
                (None, Some(_)) => tally.add(Unit::Cell, 1),
                (None, None) => continue,
            }
            cells.push(CellChange { at: *place, before: b.cloned(), after: a.cloned() });
        }
        let mark = if cells.is_empty() { Mark::Same } else { Mark::Changed };
        marked.push(SheetChanges { name: sheet.name.clone(), mark, cells });
    }
    for sheet in old.iter().filter(|s| named(new, &s.name).is_none()) {
        tally.remove(Unit::Sheet, 1);
        marked.push(SheetChanges { name: sheet.name.clone(), mark: Mark::Removed, cells: Vec::new() });
    }
    let counts = tally.finish();
    let outcome = if counts.is_empty() { Outcome::SameText } else { Outcome::Text };
    Changes { outcome, counts, marked: Marked::Sheets(marked) }
}

/// A run of removed and added blocks: alike pairs become changed blocks, the rest stay removed
/// or added, in order.
fn pair_up(
    unit: Unit,
    old: &[Block],
    new: &[Block],
    deadline: Instant,
    tally: &mut Tally,
    out: &mut Vec<MarkedBlock>,
) {
    for pair in pair_in_order(old, new, |a, b| alike(a, b, deadline)) {
        out.push(match pair {
            Pair::Both(a, b) => changed(unit, a, b, deadline, tally),
            Pair::Old(a) => removed(unit, a, tally),
            Pair::New(b) => added(unit, b, tally),
        });
    }
}

enum Pair<'a, T> {
    Both(&'a T, &'a T),
    Old(&'a T),
    New(&'a T),
}

/// Walks a removed run and an added run together, pairing alike items in order; an item with no
/// alike partner close ahead stays removed or added.
fn pair_in_order<'a, T>(old: &'a [T], new: &'a [T], alike: impl Fn(&T, &T) -> bool) -> Vec<Pair<'a, T>> {
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && alike(&old[i], &new[j]) {
            out.push(Pair::Both(&old[i], &new[j]));
            i += 1;
            j += 1;
        } else if i < old.len() && new[j..].iter().take(LOOK_AHEAD).any(|b| alike(&old[i], b)) {
            out.push(Pair::New(&new[j]));
            j += 1;
        } else if i < old.len() {
            out.push(Pair::Old(&old[i]));
            i += 1;
        } else {
            out.push(Pair::New(&new[j]));
            j += 1;
        }
    }
    out
}

fn alike(a: &Block, b: &Block, deadline: Instant) -> bool {
    if Instant::now() >= deadline {
        return false;
    }
    match (a, b) {
        (Block::Paragraph { section: s1, text: t1, .. }, Block::Paragraph { section: s2, text: t2, .. }) => {
            s1 == s2 && similar_text(t1, t2, deadline)
        }
        // Two tables in the same place are one table edited; its rows are matched in `changed`.
        (Block::Table { section: s1, .. }, Block::Table { section: s2, .. }) => s1 == s2,
        _ => false,
    }
}

/// Alike by words, or, for short runs of text without spaces such as `y,2` and `y,3`, by letters.
fn similar_text(a: &str, b: &str, deadline: Instant) -> bool {
    // Too long to measure cheaply: a long paragraph in the place of another is one edited.
    if a.len().max(b.len()) > LONG {
        return a.len().min(b.len()) > LONG;
    }
    let config = || {
        let mut c = TextDiff::configure();
        c.algorithm(Algorithm::Patience).deadline(deadline);
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
fn changed(unit: Unit, old: &Block, new: &Block, deadline: Instant, tally: &mut Tally) -> MarkedBlock {
    match (old, new) {
        (Block::Paragraph { text: a, .. }, Block::Paragraph { section, style, text: b }) => {
            tally.change(unit, 1);
            MarkedBlock::Paragraph {
                section: *section,
                style: *style,
                mark: Mark::Changed,
                spans: words(a, b, deadline),
            }
        }
        (Block::Table { rows: a, .. }, Block::Table { section, rows: b }) => {
            let rows = changed_rows(a, b, deadline, tally);
            let mark =
                if rows.iter().flatten().any(|c| c.mark != Mark::Same) { Mark::Changed } else { Mark::Same };
            MarkedBlock::Table { section: *section, mark, rows }
        }
        _ => unreachable!("only alike blocks are paired"),
    }
}

/// The rows of an edited table: unchanged rows kept, alike rows compared cell by cell, and rows
/// added or removed counted by their cells. Removed rows stay in place, marked removed.
fn changed_rows(
    a: &[Vec<String>],
    b: &[Vec<String>],
    deadline: Instant,
    tally: &mut Tally,
) -> Vec<Vec<MarkedCell>> {
    let key = |r: &Vec<String>| r.join("\u{1f}");
    let (ka, kb): (Vec<String>, Vec<String>) = (a.iter().map(key).collect(), b.iter().map(key).collect());
    let whole = |r: &Vec<String>, mark: Mark, tally: &mut Tally| -> Vec<MarkedCell> {
        match mark {
            Mark::Added => tally.add(Unit::Cell, r.len() as u32),
            Mark::Removed => tally.remove(Unit::Cell, r.len() as u32),
            Mark::Same | Mark::Changed => {}
        }
        r.iter()
            .map(|c| {
                let span = match mark {
                    Mark::Added => Span::Added(c.clone()),
                    Mark::Removed => Span::Removed(c.clone()),
                    Mark::Same | Mark::Changed => Span::Same(c.clone()),
                };
                MarkedCell { mark, spans: vec![span] }
            })
            .collect()
    };
    // Rows of the same width sharing at least half their cells are one row edited.
    let alike_rows = |x: &Vec<String>, y: &Vec<String>| {
        x.len() == y.len() && x.iter().zip(y).filter(|(p, q)| p == q).count() * 2 >= x.len()
    };
    let mut rows = Vec::new();
    for op in capture_diff_slices_deadline(Algorithm::Patience, &ka, &kb, Some(deadline)) {
        let (_, o, n) = op.as_tag_tuple();
        if let DiffOp::Equal { .. } = op {
            rows.extend(b[n].iter().map(|r| whole(r, Mark::Same, tally)));
            continue;
        }
        for pair in pair_in_order(&a[o], &b[n], alike_rows) {
            rows.push(match pair {
                Pair::Both(ra, rb) => ra
                    .iter()
                    .zip(rb)
                    .map(|(ca, cb)| {
                        if ca == cb {
                            MarkedCell { mark: Mark::Same, spans: vec![Span::Same(cb.clone())] }
                        } else {
                            tally.change(Unit::Cell, 1);
                            MarkedCell { mark: Mark::Changed, spans: words(ca, cb, deadline) }
                        }
                    })
                    .collect(),
                Pair::Old(ra) => whole(ra, Mark::Removed, tally),
                Pair::New(rb) => whole(rb, Mark::Added, tally),
            });
        }
    }
    rows
}

/// `after`'s words, with those not in `before` added and those gone shown removed.
pub(crate) fn words(before: &str, after: &str, deadline: Instant) -> Vec<Span> {
    let diff =
        TextDiff::configure().algorithm(Algorithm::Patience).deadline(deadline).diff_words(before, after);
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
    fn a_row_added_to_a_table_counts_its_cells() {
        let before = table(&[&["a", "b"], &["c", "d"], &["Total", "9"]]);
        let after = table(&[&["a", "b"], &["c", "d"], &["e", "f"], &["Total", "10"]]);
        let c = compare(
            &Reading::Text { unit: Unit::Paragraph, blocks: vec![before] },
            &Reading::Text { unit: Unit::Paragraph, blocks: vec![after] },
        );
        assert_eq!(c.counts, vec![count(Unit::Cell, 2, 0, 1)]);
        let Marked::Text(blocks) = &c.marked else { panic!() };
        let MarkedBlock::Table { rows, .. } = &blocks[0] else { panic!() };
        assert_eq!(rows.len(), 4);
        assert_eq!((rows[2][0].mark, rows[3][1].mark), (Mark::Added, Mark::Changed));
    }

    #[test]
    fn a_removed_row_counts_its_cells() {
        let c = compare(
            &Reading::Text {
                unit: Unit::Paragraph,
                blocks: vec![table(&[&["a", "b"], &["c", "d"], &["e", "f"]])],
            },
            &Reading::Text { unit: Unit::Paragraph, blocks: vec![table(&[&["a", "b"], &["e", "f"]])] },
        );
        assert_eq!(c.counts, vec![count(Unit::Cell, 0, 2, 0)]);
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
    fn past_the_deadline_the_counts_are_unknown() {
        let before = text(Unit::Paragraph, &["Support, 12 months"]);
        let after = text(Unit::Paragraph, &["Support, 18 months"]);
        let c = compare_until(&before, &after, Instant::now());
        assert_eq!((c.outcome, c.counts.len()), (Outcome::Unknown, 0));
        assert!(matches!(c.marked, Marked::Text(_)), "the page can still be shown");
    }

    #[test]
    fn a_long_paragraph_with_one_edit_is_one_changed() {
        let long = "word ".repeat(5_000);
        let c = compare(&text(Unit::Paragraph, &[&long]), &text(Unit::Paragraph, &[&format!("{long}more")]));
        assert_eq!(c.counts, vec![count(Unit::Paragraph, 0, 0, 1)]);
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

    fn sheet(name: &str, cells: &[(&str, &str)]) -> crate::Sheet {
        crate::Sheet {
            name: name.into(),
            cells: cells.iter().map(|(a, v)| (crate::CellRef::parse(a).unwrap(), v.to_string())).collect(),
        }
    }

    fn cell(a1: &str, before: Option<&str>, after: Option<&str>) -> CellChange {
        CellChange {
            at: crate::CellRef::parse(a1).unwrap(),
            before: before.map(str::to_string),
            after: after.map(str::to_string),
        }
    }

    #[test]
    fn a_changed_cell_shows_before_and_after() {
        let before =
            Reading::Sheets(vec![sheet("Costs", &[("C7", "1800"), ("C9", "10200"), ("A1", "Item")])]);
        let after = Reading::Sheets(vec![sheet("Costs", &[("C9", "10800"), ("C7", "2400"), ("A1", "Item")])]);
        let c = compare(&before, &after);
        assert_eq!(c.counts, vec![count(Unit::Cell, 0, 0, 2)]);
        assert_eq!(
            c.marked,
            Marked::Sheets(vec![SheetChanges {
                name: "Costs".into(),
                mark: Mark::Changed,
                cells: vec![cell("C7", Some("1800"), Some("2400")), cell("C9", Some("10200"), Some("10800"))],
            }])
        );
    }

    #[test]
    fn a_new_sheet_counts_as_one_sheet() {
        let c = compare(
            &Reading::Sheets(vec![sheet("A", &[("A1", "1")])]),
            &Reading::Sheets(vec![sheet("A", &[("A1", "1")]), sheet("B", &[("A1", "x"), ("A2", "y")])]),
        );
        assert_eq!(c.counts, vec![count(Unit::Sheet, 1, 0, 0)]);
        let Marked::Sheets(sheets) = &c.marked else { panic!() };
        assert_eq!((sheets[1].mark, sheets[1].cells.len()), (Mark::Added, 0));
    }

    #[test]
    fn a_removed_sheet_counts_as_one_sheet() {
        let c = compare(
            &Reading::Sheets(vec![sheet("A", &[]), sheet("Old", &[("A1", "1")])]),
            &Reading::Sheets(vec![sheet("A", &[])]),
        );
        assert_eq!(c.counts, vec![count(Unit::Sheet, 0, 1, 0)]);
    }

    #[test]
    fn cleared_and_filled_cells_are_removed_and_added() {
        let c = compare(
            &Reading::Sheets(vec![sheet("A", &[("A1", "1")])]),
            &Reading::Sheets(vec![sheet("A", &[("B2", "2")])]),
        );
        assert_eq!(c.counts, vec![count(Unit::Cell, 1, 1, 0)]);
    }

    #[test]
    fn equal_sheets_are_same_text() {
        let r = Reading::Sheets(vec![sheet("A", &[("A1", "1")])]);
        let c = compare(&r, &r);
        assert_eq!((c.outcome, c.counts.len()), (Outcome::SameText, 0));
    }
}
