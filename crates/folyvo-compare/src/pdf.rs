//! PDFs: the words of each page. The text itself comes from the platform (PDFKit on the Mac).

use crate::{Budget, Limits, Reading};

/// The pages' text as a reading, whitespace squashed to single spaces, within the text limit.
pub fn pdf_reading(pages: Vec<String>, limits: &Limits) -> Reading {
    let mut budget = Budget::new(limits);
    let mut out = Vec::with_capacity(pages.len());
    for page in pages {
        let squashed = page.split_whitespace().collect::<Vec<_>>().join(" ");
        if let Err(why) = budget.add_text(squashed.len()) {
            return Reading::Unreadable(why);
        }
        out.push(squashed);
    }
    Reading::Pages(out)
}

/// Whether two files are the same kind of document (Word, Excel, PDF, the same kind of text),
/// judged as `read` judges them. Two different kinds are never compared.
pub fn same_kind(a: &std::path::Path, b: &std::path::Path, name_hint: &str) -> bool {
    crate::sniff::sniff(a, name_hint) == crate::sniff::sniff(b, name_hint)
}

/// Whether the file at `path` is a PDF, by its first bytes.
pub fn is_pdf(path: &std::path::Path) -> bool {
    crate::sniff::sniff(path, "") == crate::sniff::Kind::Pdf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compare, Count, Limits, Mark, Marked, Reading, Span, Unit, Unreadable};

    fn pages(texts: &[&str]) -> Reading {
        pdf_reading(texts.iter().map(|t| t.to_string()).collect(), &Limits::DEFAULT)
    }

    #[test]
    fn page_text_is_squashed_to_single_spaces() {
        assert_eq!(pages(&["  Total\n\n€10,200 \t net "]), Reading::Pages(vec!["Total €10,200 net".into()]));
    }

    #[test]
    fn a_changed_page_counts_one_page() {
        let c = compare(&pages(&["Intro", "Total €10,200"]), &pages(&["Intro", "Total €10,800"]));
        assert_eq!(c.counts, vec![Count { unit: Unit::Page, added: 0, removed: 0, changed: 1 }]);
    }

    #[test]
    fn an_extra_page_is_added() {
        let c = compare(&pages(&["Intro"]), &pages(&["Intro", "Appendix"]));
        assert_eq!(c.counts, vec![Count { unit: Unit::Page, added: 1, removed: 0, changed: 0 }]);
        let c = compare(&pages(&["Intro", "Appendix"]), &pages(&["Intro"]));
        assert_eq!(c.counts, vec![Count { unit: Unit::Page, added: 0, removed: 1, changed: 0 }]);
    }

    #[test]
    fn pages_compare_word_by_word() {
        let c = compare(&pages(&["Support 12 months"]), &pages(&["Support 18 months"]));
        let Marked::Pages(p) = &c.marked else { panic!("{:?}", c.marked) };
        assert_eq!((p[0].number, p[0].mark), (1, Mark::Changed));
        assert!(p[0].spans.contains(&Span::Removed("12".into())));
        assert!(p[0].spans.contains(&Span::Added("18".into())));
    }

    #[test]
    fn pdf_text_over_the_limit_is_too_large() {
        let limits = Limits { text_bytes: 4, ..Limits::DEFAULT };
        assert_eq!(
            pdf_reading(vec!["abc".into(), "def".into()], &limits),
            Reading::Unreadable(Unreadable::TooLarge)
        );
    }

    #[test]
    fn a_pdf_is_known_by_its_first_bytes() {
        let d = tempfile::tempdir().unwrap();
        let (pdf, txt) = (d.path().join("a"), d.path().join("b"));
        std::fs::write(&pdf, b"%PDF-1.4\n").unwrap();
        std::fs::write(&txt, b"hello").unwrap();
        assert!(crate::is_pdf(&pdf));
        assert!(!crate::is_pdf(&txt));
    }
}
