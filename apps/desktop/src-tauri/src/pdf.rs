//! The text of each page of a PDF, read with PDFKit.

use std::path::Path;

use folyvo_compare::{Limits, Unreadable};

/// More pages than this is not a document a person compares by eye.
pub const MAX_PAGES: usize = 500;

/// Each page's text, in order.
pub fn page_texts(path: &Path, limits: &Limits) -> Result<Vec<String>, Unreadable> {
    page_texts_up_to(path, limits, MAX_PAGES)
}

#[cfg(target_os = "macos")]
pub(crate) fn page_texts_up_to(
    path: &Path,
    limits: &Limits,
    max_pages: usize,
) -> Result<Vec<String>, Unreadable> {
    use objc2::AnyThread;
    use objc2_foundation::{NSString, NSURL};
    use objc2_pdf_kit::PDFDocument;

    let started = std::time::Instant::now();
    if std::fs::metadata(path).map_err(|_| Unreadable::Malformed)?.len() > limits.file_bytes {
        return Err(Unreadable::TooLarge);
    }
    let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().ok_or(Unreadable::Malformed)?));
    // SAFETY: a file URL; PDFKit reads the file and returns nil when it is not a PDF it can open.
    let doc = unsafe { PDFDocument::initWithURL(PDFDocument::alloc(), &url) }.ok_or(Unreadable::Malformed)?;
    // SAFETY: plain queries on a document this thread owns.
    let count = unsafe { doc.pageCount() };
    if count > max_pages {
        return Err(Unreadable::TooLarge);
    }
    let mut pages = Vec::with_capacity(count);
    let mut text_bytes = 0;
    for i in 0..count {
        if started.elapsed() > limits.time {
            return Err(Unreadable::TooSlow);
        }
        // SAFETY: `i` is below the page count.
        let text = unsafe { doc.pageAtIndex(i).and_then(|p| p.string()) };
        // Measured before it is copied out, so the text kept never passes the limit.
        text_bytes += text.as_ref().map_or(0, |t| t.len());
        if text_bytes > limits.text_bytes {
            return Err(Unreadable::TooLarge);
        }
        pages.push(text.map(|t| t.to_string()).unwrap_or_default());
    }
    Ok(pages)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn page_texts_up_to(
    _path: &Path,
    _limits: &Limits,
    _max_pages: usize,
) -> Result<Vec<String>, Unreadable> {
    Err(Unreadable::NotSupported)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::{Path, PathBuf};

    /// A PDF with one line of Helvetica text per page.
    pub(crate) fn pdf(dir: &Path, name: &str, pages: &[&str]) -> PathBuf {
        let n = pages.len();
        let font = 3 + 2 * n;
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            format!(
                "<< /Type /Pages /Kids [{}] /Count {n} >>",
                (0..n).map(|i| format!("{} 0 R", 3 + 2 * i)).collect::<Vec<_>>().join(" ")
            ),
        ];
        for (i, text) in pages.iter().enumerate() {
            let content = format!("BT /F1 18 Tf 20 100 Td ({text}) Tj ET");
            objects.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] /Contents {} 0 R /Resources << /Font << /F1 {font} 0 R >> >> >>",
                4 + 2 * i
            ));
            objects.push(format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()));
        }
        objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{body}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").bytes());
        }
        out.extend(
            format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1)
                .bytes(),
        );
        let path = dir.join(name);
        std::fs::write(&path, out).unwrap();
        path
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn page_texts_come_back_in_order() {
        let d = tempfile::tempdir().unwrap();
        let p = pdf(d.path(), "a.pdf", &["Intro", "Total 10200"]);
        let pages = super::page_texts(&p, &folyvo_compare::Limits::DEFAULT).unwrap();
        assert_eq!(pages.iter().map(|p| p.trim()).collect::<Vec<_>>(), vec!["Intro", "Total 10200"]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn too_many_pages_is_too_large() {
        let d = tempfile::tempdir().unwrap();
        let p = pdf(d.path(), "a.pdf", &["a", "b", "c"]);
        assert_eq!(
            super::page_texts_up_to(&p, &folyvo_compare::Limits::DEFAULT, 2),
            Err(folyvo_compare::Unreadable::TooLarge)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn text_over_the_limit_stops_the_reading() {
        let d = tempfile::tempdir().unwrap();
        let p = pdf(d.path(), "a.pdf", &["Intro", "Total"]);
        let limits = folyvo_compare::Limits { text_bytes: 6, ..folyvo_compare::Limits::DEFAULT };
        assert_eq!(super::page_texts(&p, &limits), Err(folyvo_compare::Unreadable::TooLarge));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn two_pdfs_differ_by_a_page() {
        use folyvo_core::engine::CompareInputs;
        use folyvo_core::model::{ChangeCount, ChangeOutcome, ChangeUnit, ContentHash, ItemId, VersionId};
        let d = tempfile::tempdir().unwrap();
        let a = pdf(d.path(), "a.pdf", &["Intro", "Total 10200"]);
        let b = pdf(d.path(), "b.pdf", &["Intro", "Total 10800"]);
        let hash = |p: &Path| ContentHash::of_bytes(&std::fs::read(p).unwrap());
        let r = crate::changes::summarise(&CompareInputs {
            item: ItemId::from_suffix("01"),
            version: VersionId::from_suffix("02"),
            after_hash: hash(&b),
            after: b,
            before: Some((VersionId::from_suffix("01"), a.clone(), hash(&a))),
            name: "Offer.pdf".into(),
        });
        assert_eq!(r.outcome, ChangeOutcome::Text);
        assert_eq!(r.counts, vec![ChangeCount { unit: ChangeUnit::Page, added: 0, removed: 0, changed: 1 }]);
    }
}
