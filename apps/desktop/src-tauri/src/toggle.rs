#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Show,
    Hide,
    Ignore,
}

/// Decides what a tray click means. Clicking the tray icon while the popover is open first takes
/// focus away from it (the popover hides itself), and only then does the click arrive. Without
/// this gate that click would see a hidden popover and open it again.
pub struct ToggleGate {
    cooldown_ms: i64,
    hidden_by_blur_at: Option<i64>,
    dialog_open: bool,
}

impl ToggleGate {
    pub fn new(cooldown_ms: i64) -> Self {
        Self { cooldown_ms, hidden_by_blur_at: None, dialog_open: false }
    }

    /// The native folder picker takes focus while it is open; the popover must not hide then.
    pub fn set_dialog_open(&mut self, open: bool) {
        self.dialog_open = open;
    }

    /// Returns whether the popover should hide.
    pub fn on_blur(&mut self, now_ms: i64) -> bool {
        if self.dialog_open {
            return false;
        }
        self.hidden_by_blur_at = Some(now_ms);
        true
    }

    pub fn on_tray_click(&mut self, now_ms: i64, visible: bool) -> TrayAction {
        if visible {
            return TrayAction::Hide;
        }
        match self.hidden_by_blur_at.take() {
            Some(at) if now_ms - at < self.cooldown_ms => TrayAction::Ignore,
            _ => TrayAction::Show,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_on_a_hidden_popover_shows_it() {
        let mut g = ToggleGate::new(300);
        assert_eq!(g.on_tray_click(1_000, false), TrayAction::Show);
    }

    #[test]
    fn a_click_on_a_visible_popover_hides_it() {
        let mut g = ToggleGate::new(300);
        assert_eq!(g.on_tray_click(1_000, true), TrayAction::Hide);
    }

    // Review Focus 1: blur hides the popover, then the same click arrives and must not reopen it.
    #[test]
    fn the_click_that_closed_the_popover_by_blur_does_not_reopen_it() {
        let mut g = ToggleGate::new(300);
        g.on_blur(1_000);
        assert_eq!(g.on_tray_click(1_120, false), TrayAction::Ignore);
        assert_eq!(g.on_tray_click(1_900, false), TrayAction::Show, "the next real click opens it again");
    }

    #[test]
    fn a_click_well_after_a_blur_shows_the_popover() {
        let mut g = ToggleGate::new(300);
        g.on_blur(1_000);
        assert_eq!(g.on_tray_click(1_500, false), TrayAction::Show);
    }

    // The native folder picker takes focus from the popover. Hiding it then would take the
    // picker's window with it.
    #[test]
    fn a_blur_while_a_dialog_is_open_keeps_the_popover() {
        let mut g = ToggleGate::new(300);
        g.set_dialog_open(true);
        assert!(!g.on_blur(1_000), "the popover stays");
        g.set_dialog_open(false);
        assert!(g.on_blur(2_000), "after the dialog closes a blur hides it again");
    }

    #[test]
    fn a_blur_that_kept_the_popover_does_not_swallow_the_next_tray_click() {
        let mut g = ToggleGate::new(300);
        g.set_dialog_open(true);
        g.on_blur(1_000);
        assert_eq!(g.on_tray_click(1_100, false), TrayAction::Show);
    }
}
