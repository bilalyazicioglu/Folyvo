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
        out.push_str(&format!(
            "v{:<3} {:<16} {:>9}  {:<12} {}\n",
            v.seq,
            bar(v.size),
            size_label(v.size),
            ago(now_ms - v.created_at),
            note(v)
        ));
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
}
