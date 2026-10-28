//! Documents made in code for tests.

use std::io::Write;
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

fn write_zip(dir: &Path, name: &str, parts: &[(&str, &[u8])], method: CompressionMethod) -> PathBuf {
    let path = dir.join(name);
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    for (part, bytes) in parts {
        zip.start_file(*part, SimpleFileOptions::default().compression_method(method)).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    path
}

/// A zip with deflated parts.
pub(crate) fn zip_file(dir: &Path, name: &str, parts: &[(&str, &[u8])]) -> PathBuf {
    write_zip(dir, name, parts, CompressionMethod::Deflated)
}

/// A zip with stored (uncompressed) parts.
pub(crate) fn zip_stored(dir: &Path, name: &str, parts: &[(&str, &[u8])]) -> PathBuf {
    write_zip(dir, name, parts, CompressionMethod::Stored)
}

/// Overwrites the first entry's method or uncompressed size in both its local and central header.
pub(crate) fn patch_first_entry(path: &Path, method: Option<u16>, size: Option<u32>) {
    let mut bytes = std::fs::read(path).unwrap();
    let local = find(&bytes, b"PK\x03\x04");
    let central = find(&bytes, b"PK\x01\x02");
    if let Some(m) = method {
        bytes[local + 8..local + 10].copy_from_slice(&m.to_le_bytes());
        bytes[central + 10..central + 12].copy_from_slice(&m.to_le_bytes());
    }
    if let Some(s) = size {
        bytes[local + 22..local + 26].copy_from_slice(&s.to_le_bytes());
        bytes[central + 24..central + 28].copy_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

fn find(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).position(|w| w == needle).unwrap()
}

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// A Word file whose body is `body_xml`, plus any other parts given whole.
pub(crate) fn docx(dir: &Path, body_xml: &str, extra: &[(&str, &str)]) -> PathBuf {
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}"><w:body>{body_xml}</w:body></w:document>"#
    );
    let mut parts: Vec<(&str, &[u8])> = vec![("word/document.xml", document.as_bytes())];
    parts.extend(extra.iter().map(|(n, x)| (*n, x.as_bytes())));
    zip_file(dir, "doc.docx", &parts)
}

/// `<w:p>` with one run of `text`.
pub(crate) fn wp(text: &str) -> String {
    format!("<w:p><w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r></w:p>")
}

/// Overwrites the entry counts in the end-of-central-directory record.
pub(crate) fn patch_entry_count(path: &Path, count: u16) {
    let mut bytes = std::fs::read(path).unwrap();
    let end = bytes.windows(4).rposition(|w| w == b"PK\x05\x06").unwrap();
    bytes[end + 8..end + 10].copy_from_slice(&count.to_le_bytes());
    bytes[end + 10..end + 12].copy_from_slice(&count.to_le_bytes());
    std::fs::write(path, bytes).unwrap();
}

const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// An Excel file. `sheets` are (name, file under `xl/worksheets/`, `<sheetData>` inner XML), in
/// workbook order; `shared` the shared strings; `styles` the whole `styles.xml`, if any.
pub(crate) fn xlsx(
    dir: &Path,
    sheets: &[(&str, &str, &str)],
    shared: &[&str],
    styles: Option<&str>,
) -> PathBuf {
    let listed: String = sheets
        .iter()
        .enumerate()
        .map(|(i, (name, _, _))| format!(r#"<sheet name="{name}" sheetId="{}" r:id="rId{}"/>"#, i + 1, i + 1))
        .collect();
    let workbook = format!(r#"<workbook xmlns="{S}" xmlns:r="{R}"><sheets>{listed}</sheets></workbook>"#);
    let rels: String = sheets
        .iter()
        .enumerate()
        .map(|(i, (_, file, _))| {
            format!(r#"<Relationship Id="rId{}" Type="{R}/worksheet" Target="worksheets/{file}"/>"#, i + 1)
        })
        .collect();
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#
    );
    let strings: String = shared.iter().map(|s| format!("<si><t>{s}</t></si>")).collect();
    let strings = format!(r#"<sst xmlns="{S}">{strings}</sst>"#);
    let mut owned: Vec<(String, String)> = vec![
        ("xl/workbook.xml".into(), workbook),
        ("xl/_rels/workbook.xml.rels".into(), rels),
        ("xl/sharedStrings.xml".into(), strings),
    ];
    for (_, file, data) in sheets {
        owned.push((
            format!("xl/worksheets/{file}"),
            format!(r#"<worksheet xmlns="{S}"><sheetData>{data}</sheetData></worksheet>"#),
        ));
    }
    if let Some(styles) = styles {
        owned.push(("xl/styles.xml".into(), styles.to_string()));
    }
    let parts: Vec<(&str, &[u8])> = owned.iter().map(|(n, x)| (n.as_str(), x.as_bytes())).collect();
    zip_file(dir, "book.xlsx", &parts)
}
