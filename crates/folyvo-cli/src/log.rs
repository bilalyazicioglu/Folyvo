//! `folyvo log`: every saved version, newest first, with its code and where copies went.

use folyvo_core::model::{ShareEvent, ShareEventKind, VersionOrigin};
use folyvo_core::{History, Version};

/// The words a person sees for a handed-out copy. Never "sent": the share sheet only tells us the
/// other app took the file.
pub fn handed_label(service: &str) -> String {
    match service {
        "mail" => "Attached in Mail".into(),
        "airdrop" => "AirDrop".into(),
        "messages" => "Messages".into(),
        "copy" => "Copied".into(),
        other => other.strip_prefix("other:").unwrap_or(other).to_owned(),
    }
}

/// `when` turns unix milliseconds into the text shown for a time.
pub fn render(history: &History, when: &dyn Fn(i64) -> String) -> String {
    let mut out = format!("{}  {} versions\n", history.name, history.versions.len());
    for (v, code) in history.versions.iter().zip(&history.codes) {
        let line = format!("{}  {}  {}  {}", when(v.created_at), code, size_label(v.size), note(v));
        out.push_str(line.trim_end());
        out.push('\n');
        for e in history.events.iter().filter(|e| e.version_id == v.id) {
            out.push_str(&format!("  {}\n", event_line(e, when)));
        }
    }
    out
}

fn size_label(size: u64) -> String {
    match size {
        0..=1_023 => format!("{size} B"),
        1_024..=1_048_575 => format!("{} KB", size / 1_024),
        _ => format!("{:.1} MB", size as f64 / 1_048_576.0),
    }
}

fn note(v: &Version) -> &str {
    match (&v.note, v.origin) {
        (Some(n), _) => n,
        (None, VersionOrigin::Restore) => "Restored",
        (None, VersionOrigin::Initial) => "First version",
        (None, VersionOrigin::Auto) => "",
    }
}

/// One copy of a version: where it went and when.
pub fn event_line(e: &ShareEvent, when: &dyn Fn(i64) -> String) -> String {
    let what = match e.kind {
        ShareEventKind::Handed => handed_label(e.service.as_deref().unwrap_or("")),
        ShareEventKind::Exported => format!("Copy saved to {}", e.target_path.as_deref().unwrap_or("?")),
    };
    format!("{what}, {}", when(e.at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use folyvo_core::model::{ContentHash, ItemId, VersionId};

    fn version(suffix: &str, seq: i64, note: Option<&str>) -> Version {
        Version {
            id: VersionId::from_suffix(suffix),
            item_id: ItemId::from_suffix("01"),
            seq,
            content_hash: ContentHash::of_bytes(suffix.as_bytes()),
            size: 2_048,
            created_at: seq * 1_000,
            origin: if seq == 1 { VersionOrigin::Initial } else { VersionOrigin::Auto },
            restored_from: None,
            note: note.map(str::to_owned),
        }
    }

    fn event(
        suffix: &str,
        kind: ShareEventKind,
        service: Option<&str>,
        target: Option<&str>,
        at: i64,
    ) -> ShareEvent {
        ShareEvent {
            item_id: ItemId::from_suffix("01"),
            version_id: VersionId::from_suffix(suffix),
            kind,
            service: service.map(str::to_owned),
            target_path: target.map(str::to_owned),
            at,
        }
    }

    fn at(ms: i64) -> String {
        format!("t{ms}")
    }

    #[test]
    fn handed_services_read_as_what_happened_never_as_sent() {
        assert_eq!(handed_label("mail"), "Attached in Mail");
        assert_eq!(handed_label("airdrop"), "AirDrop");
        assert_eq!(handed_label("messages"), "Messages");
        assert_eq!(handed_label("copy"), "Copied");
        assert_eq!(handed_label("other:Blip"), "Blip");
    }

    #[test]
    fn each_version_is_a_line_with_time_code_size_and_note_newest_first() {
        let h = History {
            name: "Teklif.docx".into(),
            versions: vec![version("bbbb2222", 2, Some("Müşteriye giden")), version("aaaa1111", 1, None)],
            codes: vec!["bbbb2222".into(), "aaaa1111".into()],
            events: vec![],
        };
        let text = render(&h, &at);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "Teklif.docx  2 versions");
        assert_eq!(lines[1], "t2000  bbbb2222  2 KB  Müşteriye giden");
        assert_eq!(lines[2], "t1000  aaaa1111  2 KB  First version");
        assert!(!text.contains("v1") && !text.contains('█'), "{text}");
    }

    #[test]
    fn copies_are_listed_under_their_version_oldest_first() {
        let h = History {
            name: "a.docx".into(),
            versions: vec![version("bbbb2222", 2, None), version("aaaa1111", 1, None)],
            codes: vec!["bbbb2222".into(), "aaaa1111".into()],
            events: vec![
                event("aaaa1111", ShareEventKind::Handed, Some("mail"), None, 1_500),
                event("aaaa1111", ShareEventKind::Exported, None, Some("/out/a.docx"), 1_700),
            ],
        };
        let text = render(&h, &at);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            &lines[2..],
            [
                "t1000  aaaa1111  2 KB  First version",
                "  Attached in Mail, t1500",
                "  Copy saved to /out/a.docx, t1700"
            ]
        );
        assert!(!text.to_lowercase().contains("sent"), "{text}");
    }
}
