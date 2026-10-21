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
}

impl ToggleGate {
    pub fn new(cooldown_ms: i64) -> Self {
        Self { cooldown_ms, hidden_by_blur_at: None }
    }

    pub fn on_blur(&mut self, now_ms: i64) {
        self.hidden_by_blur_at = Some(now_ms);
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
}
