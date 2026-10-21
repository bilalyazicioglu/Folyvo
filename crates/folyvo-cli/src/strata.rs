use folyvo_core::{History, Version};

const MAX_BAR: usize = 16;

/// Width grows with log2 of the size: 1 byte is one block, about 64 KB is eight, 4 GB and above is sixteen.
pub fn bar(size: u64) -> String {
    let bits = 64 - size.leading_zeros() as usize; // 0 for size 0
    let width = (bits / 2).clamp(1, MAX_BAR);
    "█".repeat(width)
}

pub fn ago(delta_ms: i64) -> String {
    let s = delta_ms.max(0) / 1000;
    match s {
        0..=44 => "just now".to_string(),
        45..=3_599 => format!("{} min ago", (s / 60).max(1)),
        3_600..=86_399 => format!("{} h ago", s / 3_600),
        _ => format!("{} days ago", s / 86_400),
    }
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
        (None, folyvo_core::model::VersionOrigin::Restore) => "Restored",
        (None, folyvo_core::model::VersionOrigin::Initial) => "First version",
        (None, folyvo_core::model::VersionOrigin::Auto) => "Autosaved",
    }
}

pub fn render(history: &History, now_ms: i64) -> String {
    let mut out = format!("{}  {} versions\n", history.name, history.versions.len());
    for v in &history.versions {
        let ready = history.share_ready.as_ref() == Some(&v.id);
        let tag = if ready { "  <- ready to share" } else { "" };
        out.push_str(&format!(
            "v{:<3} {:<16} {:>9}  {:<12} {}{}\n",
            v.seq,
            bar(v.size),
            size_label(v.size),
            ago(now_ms - v.created_at),
            note(v),
            tag
        ));
    }
    let ready = history.versions.iter().find(|v| history.share_ready.as_ref() == Some(&v.id));
    if let (Some(ready), Some(latest)) = (ready, history.versions.first()) {
        // Same rule as the core: compare content, so restoring the marked content is "ready" again.
        if latest.content_hash != ready.content_hash {
            out.push_str(&format!("Changed since v{}, your share-ready version.\n", ready.seq));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_grows_with_size_on_a_log_scale_and_never_vanishes() {
        assert_eq!(bar(0).chars().count(), 1);
        assert!(bar(1_000).chars().count() < bar(1_000_000).chars().count());
        assert_eq!(bar(u64::MAX).chars().count(), 16);
    }

    #[test]
    fn ago_uses_the_largest_whole_unit() {
        assert_eq!(ago(20_000), "just now");
        assert_eq!(ago(2 * 60_000), "2 min ago");
        assert_eq!(ago(3 * 3_600_000), "3 h ago");
        assert_eq!(ago(2 * 86_400_000), "2 days ago");
    }

    fn version(seq: i64, size: u64) -> Version {
        use folyvo_core::model::{ContentHash, ItemId, VersionId, VersionOrigin};
        Version {
            id: VersionId::from_suffix(&format!("{seq:02x}")),
            item_id: ItemId::from_suffix("01"),
            seq,
            content_hash: ContentHash::of_bytes(&[seq as u8]),
            size,
            created_at: 0,
            origin: VersionOrigin::Auto,
            restored_from: None,
            note: None,
        }
    }

    #[test]
    fn the_share_ready_layer_is_labelled_and_a_newer_layer_is_called_out() {
        let versions = vec![version(2, 500), version(1, 400)];
        let h = History { name: "a.docx".into(), share_ready: Some(versions[1].id.clone()), versions };
        let text = render(&h, 0);
        let line = text.lines().find(|l| l.contains("ready to share")).unwrap();
        assert!(line.starts_with("v1"), "{text}");
        assert!(text.contains("Changed since v1, your share-ready version."), "{text}");
    }

    #[test]
    fn no_callout_when_the_latest_layer_is_the_share_ready_one() {
        let versions = vec![version(1, 400)];
        let h = History { name: "a.docx".into(), share_ready: Some(versions[0].id.clone()), versions };
        assert!(!render(&h, 0).contains("Changed since"));
    }

    #[test]
    fn no_callout_after_restoring_the_share_ready_content() {
        let ready = version(1, 400);
        let mut restored = version(2, 400);
        restored.content_hash = ready.content_hash.clone();
        let h = History {
            name: "a.docx".into(),
            share_ready: Some(ready.id.clone()),
            versions: vec![restored, ready],
        };
        assert!(!render(&h, 0).contains("Changed since"));
    }
}
