//! Excel workbooks: every sheet's cells by address, with the values Excel last saved.

use std::collections::{BTreeMap, HashMap};

use crate::archive::Archive;
use crate::xml::{attr, Ev, Xml};
use crate::{Budget, CellRef, Reading, Sheet, Unreadable};

pub(crate) fn read_excel(archive: &mut Archive, budget: &mut Budget) -> Reading {
    match read_book(archive, budget) {
        Ok(sheets) => Reading::Sheets(sheets),
        Err(why) => Reading::Unreadable(why),
    }
}

fn read_book(archive: &mut Archive, budget: &mut Budget) -> Result<Vec<Sheet>, Unreadable> {
    let workbook = archive.read_part("xl/workbook.xml", budget)?.ok_or(Unreadable::Malformed)?;
    let (listed, date1904) = workbook_sheets(&workbook, budget)?;
    let rels = match archive.read_part("xl/_rels/workbook.xml.rels", budget)? {
        Some(xml) => relationships(&xml, budget)?,
        None => HashMap::new(),
    };
    let shared = match archive.read_part("xl/sharedStrings.xml", budget)? {
        Some(xml) => shared_strings(&xml, budget)?,
        None => Vec::new(),
    };
    let dates = match archive.read_part("xl/styles.xml", budget)? {
        Some(xml) => date_styles(&xml, budget)?,
        None => Vec::new(),
    };
    let mut sheets = Vec::new();
    for (name, id) in listed {
        let Some(target) = rels.get(&id) else { continue };
        let part = match target.strip_prefix('/') {
            Some(absolute) => absolute.to_string(),
            None => format!("xl/{target}"),
        };
        let Some(xml) = archive.read_part(&part, budget)? else { continue };
        let cells = sheet_cells(&xml, &shared, &dates, date1904, budget)?;
        sheets.push(Sheet { name, cells });
    }
    Ok(sheets)
}

/// The sheets as the workbook lists them (name, relationship id), and whether dates count from 1904.
fn workbook_sheets(xml: &[u8], budget: &mut Budget) -> Result<(Vec<(String, String)>, bool), Unreadable> {
    let mut reader = Xml::new(xml);
    let (mut sheets, mut date1904) = (Vec::new(), false);
    while let Some(ev) = reader.next(budget)? {
        if let Ev::Start(name, attrs) | Ev::Empty(name, attrs) = ev {
            match name.as_str() {
                "sheet" => {
                    if let (Some(n), Some(id)) = (attr(&attrs, "name"), attr(&attrs, "id")) {
                        sheets.push((n.to_string(), id.to_string()));
                    }
                }
                "workbookPr" => date1904 = matches!(attr(&attrs, "date1904"), Some("1" | "true")),
                _ => {}
            }
        }
    }
    Ok((sheets, date1904))
}

fn relationships(xml: &[u8], budget: &mut Budget) -> Result<HashMap<String, String>, Unreadable> {
    let mut reader = Xml::new(xml);
    let mut rels = HashMap::new();
    while let Some(ev) = reader.next(budget)? {
        if let Ev::Start(name, attrs) | Ev::Empty(name, attrs) = ev {
            if name == "Relationship" {
                if let (Some(id), Some(target)) = (attr(&attrs, "Id"), attr(&attrs, "Target")) {
                    rels.insert(id.to_string(), target.to_string());
                }
            }
        }
    }
    Ok(rels)
}

/// Each shared string is its `t` text, rich-text runs joined, phonetic hints (`rPh`) left out.
fn shared_strings(xml: &[u8], budget: &mut Budget) -> Result<Vec<String>, Unreadable> {
    let mut reader = Xml::new(xml);
    let (mut strings, mut current, mut in_t, mut in_phonetic) = (Vec::new(), String::new(), false, 0usize);
    while let Some(ev) = reader.next(budget)? {
        match ev {
            Ev::Start(name, _) => match name.as_str() {
                "t" => in_t = true,
                "rPh" => in_phonetic += 1,
                _ => {}
            },
            Ev::End(name) => match name.as_str() {
                "t" => in_t = false,
                "rPh" => in_phonetic = in_phonetic.saturating_sub(1),
                "si" => {
                    budget.add_text(current.len())?;
                    strings.push(std::mem::take(&mut current));
                }
                _ => {}
            },
            Ev::Empty(name, _) if name == "si" => strings.push(String::new()),
            Ev::Text(text) if in_t && in_phonetic == 0 => current.push_str(&text),
            _ => {}
        }
    }
    Ok(strings)
}

/// For each cell style index, whether it shows a number as a date (built-in formats 14–22, 45–47).
fn date_styles(xml: &[u8], budget: &mut Budget) -> Result<Vec<bool>, Unreadable> {
    let mut reader = Xml::new(xml);
    let (mut styles, mut in_xfs) = (Vec::new(), false);
    while let Some(ev) = reader.next(budget)? {
        match ev {
            Ev::Start(name, _) if name == "cellXfs" => in_xfs = true,
            Ev::End(name) if name == "cellXfs" => in_xfs = false,
            Ev::Start(name, attrs) | Ev::Empty(name, attrs) if in_xfs && name == "xf" => {
                let id = attr(&attrs, "numFmtId").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
                styles.push(matches!(id, 14..=22 | 45..=47));
            }
            _ => {}
        }
    }
    Ok(styles)
}

/// One cell being read.
#[derive(Default)]
struct Cell {
    at: Option<CellRef>,
    kind: String,
    style: usize,
    value: String,
}

fn sheet_cells(
    xml: &[u8],
    shared: &[String],
    dates: &[bool],
    date1904: bool,
    budget: &mut Budget,
) -> Result<BTreeMap<CellRef, String>, Unreadable> {
    let mut reader = Xml::new(xml);
    let mut cells = BTreeMap::new();
    let mut cell: Option<Cell> = None;
    let mut reading = false; // inside `<v>`, or the `<t>` of an inline string
    while let Some(ev) = reader.next(budget)? {
        match ev {
            Ev::Start(name, attrs) if name == "c" => {
                cell = Some(Cell {
                    at: attr(&attrs, "r").and_then(CellRef::parse),
                    kind: attr(&attrs, "t").unwrap_or("n").to_string(),
                    style: attr(&attrs, "s").and_then(|s| s.parse().ok()).unwrap_or(0),
                    value: String::new(),
                })
            }
            Ev::Start(name, _) if name == "v" || name == "t" => reading = cell.is_some(),
            Ev::End(name) if name == "v" || name == "t" => reading = false,
            Ev::Text(text) if reading => {
                if let Some(c) = cell.as_mut() {
                    c.value.push_str(&text);
                }
            }
            Ev::End(name) if name == "c" => {
                let Some(c) = cell.take() else { continue };
                let (Some(at), Some(value)) = (c.at, shown(&c, shared, dates, date1904)?) else { continue };
                budget.add_cell()?;
                budget.add_text(value.len())?;
                cells.insert(at, value);
            }
            _ => {}
        }
    }
    Ok(cells)
}

/// The value a cell shows, or `None` when it is empty.
fn shown(c: &Cell, shared: &[String], dates: &[bool], date1904: bool) -> Result<Option<String>, Unreadable> {
    if c.value.is_empty() {
        return Ok(None);
    }
    let value = match c.kind.as_str() {
        "s" => {
            let i: usize = c.value.trim().parse().map_err(|_| Unreadable::Malformed)?;
            shared.get(i).cloned().ok_or(Unreadable::Malformed)?
        }
        "b" => if c.value.trim() == "1" { "TRUE" } else { "FALSE" }.to_string(),
        "n" if dates.get(c.style).copied().unwrap_or(false) => {
            as_date(&c.value, date1904).unwrap_or_else(|| c.value.clone())
        }
        _ => c.value.clone(),
    };
    Ok(Some(value))
}

/// A whole-number date serial as `YYYY-MM-DD`; anything else is left as it is.
fn as_date(value: &str, date1904: bool) -> Option<String> {
    let serial: f64 = value.trim().parse().ok()?;
    if serial.fract() != 0.0 || !(1.0..=2_958_465.0).contains(&serial) {
        return None;
    }
    // Serials count days from 1899-12-30 (which absorbs Excel's 1900 leap-year mistake for any
    // date after February 1900), or from 1904-01-01.
    let epoch = if date1904 { days_from_civil(1904, 1, 1) } else { days_from_civil(1899, 12, 30) };
    let (y, m, d) = civil_from_days(epoch + serial as i64);
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use crate::testkit::xlsx;
    use crate::{read, read_with, CellRef, Limits, Reading, Sheet, Unreadable};

    fn sheets_of(path: &std::path::Path) -> Vec<Sheet> {
        match read(path, "book.xlsx") {
            Reading::Sheets(s) => s,
            other => panic!("{other:?}"),
        }
    }

    fn at(sheet: &Sheet, a1: &str) -> Option<String> {
        sheet.cells.get(&CellRef::parse(a1).unwrap()).cloned()
    }

    const DATE_STYLES: &str = r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="14"/></cellXfs></styleSheet>"#;

    #[test]
    fn cells_read_by_address_with_shared_and_inline_strings() {
        let d = tempfile::tempdir().unwrap();
        let data = r#"<row r="1"><c r="D1" t="inlineStr"><is><t>Note</t></is></c><c r="E1" t="b"><v>1</v></c></row>
                      <row r="7"><c r="B7" t="s"><v>0</v></c><c r="C7"><v>1800</v></c><c r="F7" t="s"/></row>"#;
        let s = sheets_of(&xlsx(d.path(), &[("Costs", "sheet1.xml", data)], &["Support, 12 months"], None));
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "Costs");
        assert_eq!(at(&s[0], "B7").as_deref(), Some("Support, 12 months"));
        assert_eq!(at(&s[0], "C7").as_deref(), Some("1800"));
        assert_eq!(at(&s[0], "D1").as_deref(), Some("Note"));
        assert_eq!(at(&s[0], "E1").as_deref(), Some("TRUE"));
        assert_eq!(at(&s[0], "F7"), None, "an empty cell is not stored");
    }

    #[test]
    fn sheets_keep_workbook_order_and_names() {
        let d = tempfile::tempdir().unwrap();
        let path = xlsx(
            d.path(),
            &[
                ("Costs", "sheet2.xml", r#"<row r="1"><c r="A1"><v>1</v></c></row>"#),
                ("Notes", "sheet1.xml", ""),
            ],
            &[],
            None,
        );
        let s = sheets_of(&path);
        assert_eq!(s.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["Costs", "Notes"]);
        assert_eq!(at(&s[0], "A1").as_deref(), Some("1"));
    }

    #[test]
    fn a_date_formatted_serial_reads_as_a_date() {
        let d = tempfile::tempdir().unwrap();
        let data = r#"<row r="1"><c r="A1" s="1"><v>46316</v></c><c r="B1" s="0"><v>46316</v></c></row>"#;
        let s = sheets_of(&xlsx(d.path(), &[("S", "sheet1.xml", data)], &[], Some(DATE_STYLES)));
        assert_eq!(at(&s[0], "A1").as_deref(), Some("2026-10-21"));
        assert_eq!(at(&s[0], "B1").as_deref(), Some("46316"));
    }

    #[test]
    fn formulas_read_as_their_saved_value() {
        let d = tempfile::tempdir().unwrap();
        let data = r#"<row r="9"><c r="C9"><f>SUM(C5:C8)</f><v>10200</v></c></row>"#;
        let s = sheets_of(&xlsx(d.path(), &[("S", "sheet1.xml", data)], &[], None));
        assert_eq!(at(&s[0], "C9").as_deref(), Some("10200"));
    }

    #[test]
    fn a_bad_shared_string_index_is_malformed() {
        let d = tempfile::tempdir().unwrap();
        let data = r#"<row r="1"><c r="A1" t="s"><v>5</v></c></row>"#;
        let path = xlsx(d.path(), &[("S", "sheet1.xml", data)], &["only one"], None);
        assert_eq!(read(&path, "book.xlsx"), Reading::Unreadable(Unreadable::Malformed));
    }

    #[test]
    fn too_many_cells_is_too_large() {
        let d = tempfile::tempdir().unwrap();
        let data = r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><v>2</v></c><c r="C1"><v>3</v></c><c r="D1"><v>4</v></c></row>"#;
        let path = xlsx(d.path(), &[("S", "sheet1.xml", data)], &[], None);
        let limits = Limits { cells: 3, ..Limits::DEFAULT };
        assert_eq!(read_with(&path, "book.xlsx", &limits), Reading::Unreadable(Unreadable::TooLarge));
    }

    #[test]
    fn a_dtd_in_a_sheet_is_malformed() {
        let d = tempfile::tempdir().unwrap();
        let path = xlsx(d.path(), &[("S", "sheet1.xml", "")], &[], None);
        let evil = crate::testkit::zip_file(
            d.path(),
            "evil.xlsx",
            &[
                ("xl/workbook.xml", br#"<!DOCTYPE w [<!ENTITY a "x">]><workbook><sheets/></workbook>"#),
                ("xl/_rels/workbook.xml.rels", b"<Relationships/>"),
            ],
        );
        assert!(matches!(read(&path, "book.xlsx"), Reading::Sheets(_)));
        assert_eq!(read(&evil, "evil.xlsx"), Reading::Unreadable(Unreadable::Malformed));
    }
}
