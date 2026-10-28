//! Telling kinds apart. The vault keeps contents without names, so documents are known by what
//! is inside them; only plain text goes by the name.

use std::io::Read;
use std::path::Path;

use crate::archive::Archive;
use crate::{text_unit, Limits, Unit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Word,
    Excel,
    Pdf,
    Text(Unit),
    Other,
}

/// What kind of document the file at `path` is; `name_hint` decides only between text and other.
pub(crate) fn sniff(path: &Path, name_hint: &str) -> Kind {
    let mut head = [0u8; 5];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    let head = &head[..n];
    if head.starts_with(b"%PDF-") {
        return Kind::Pdf;
    }
    if head.starts_with(b"PK\x03\x04") {
        return match Archive::open(path, &Limits::DEFAULT) {
            Ok(zip) if zip.has("word/document.xml") => Kind::Word,
            Ok(zip) if zip.has("xl/workbook.xml") => Kind::Excel,
            _ => Kind::Other,
        };
    }
    text_unit(name_hint).map_or(Kind::Other, Kind::Text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::zip_file;
    use crate::Unit;

    fn kind_of(name: &str, bytes: &[u8]) -> Kind {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("object");
        std::fs::write(&p, bytes).unwrap();
        sniff(&p, name)
    }

    #[test]
    fn a_pdf_is_known_by_its_first_bytes() {
        assert_eq!(kind_of("x.txt", b"%PDF-1.7\n1 0 obj"), Kind::Pdf);
    }

    #[test]
    fn a_word_zip_is_word_whatever_its_name() {
        let d = tempfile::tempdir().unwrap();
        let word = zip_file(d.path(), "cv.zip", &[("word/document.xml", b"<x/>")]);
        let excel = zip_file(d.path(), "b.txt", &[("xl/workbook.xml", b"<x/>")]);
        let other = zip_file(d.path(), "c.docx", &[("hello.txt", b"hi")]);
        assert_eq!(sniff(&word, "cv.zip"), Kind::Word);
        assert_eq!(sniff(&excel, "b.txt"), Kind::Excel);
        assert_eq!(sniff(&other, "c.docx"), Kind::Other);
    }

    #[test]
    fn text_falls_back_to_the_name() {
        assert_eq!(kind_of("a.md", b"hello"), Kind::Text(Unit::Paragraph));
        assert_eq!(kind_of("a.yaml", b"hello"), Kind::Text(Unit::Line));
        assert_eq!(kind_of("a.bin", b"hello"), Kind::Other);
    }
}
