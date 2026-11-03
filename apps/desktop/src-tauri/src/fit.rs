//! The popover window follows the height of what it shows, within limits.

pub const POPOVER_WIDTH: f64 = 380.0;
/// Clear space beside and below the glass: the glass draws its own shadow, and the window's bounds
/// would cut it off. Matches the popover's margin in `app.css`.
pub const POPOVER_SHADOW_ROOM: f64 = 40.0;
/// Clear space above the glass, the air under the menu bar. The window cannot go up into the menu
/// bar, and the shadow there would be hidden behind it anyway. Matches `app.css` too.
pub const POPOVER_GAP: f64 = 6.0;
pub const POPOVER_WINDOW_WIDTH: f64 = POPOVER_WIDTH + 2.0 * POPOVER_SHADOW_ROOM;
const MIN_HEIGHT: f64 = 160.0;
const MAX_HEIGHT: f64 = 640.0;

/// Window height for `content_px` of content, with the clear space above and below the glass.
pub fn popover_height(content_px: f64) -> f64 {
    let content = if content_px.is_finite() { content_px.clamp(MIN_HEIGHT, MAX_HEIGHT) } else { MIN_HEIGHT };
    content + POPOVER_GAP + POPOVER_SHADOW_ROOM
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_popover_follows_its_content_within_limits() {
        assert_eq!(popover_height(100.0), 206.0);
        assert_eq!(popover_height(420.0), 466.0);
        assert_eq!(popover_height(900.0), 686.0);
        assert_eq!(popover_height(f64::NAN), 206.0, "a broken measurement never collapses the window");
    }

    #[test]
    fn the_window_has_room_for_the_shadow_on_each_side() {
        assert_eq!(POPOVER_WINDOW_WIDTH, POPOVER_WIDTH + 2.0 * POPOVER_SHADOW_ROOM);
    }
}
