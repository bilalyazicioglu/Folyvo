//! The popover window follows the height of what it shows, within limits.

pub const POPOVER_WIDTH: f64 = 380.0;
const MIN_HEIGHT: f64 = 160.0;
const MAX_HEIGHT: f64 = 640.0;

/// Window height for `content_px` of content.
pub fn popover_height(content_px: f64) -> f64 {
    if content_px.is_finite() {
        content_px.clamp(MIN_HEIGHT, MAX_HEIGHT)
    } else {
        MIN_HEIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_popover_follows_its_content_within_limits() {
        assert_eq!(popover_height(100.0), 160.0);
        assert_eq!(popover_height(420.0), 420.0);
        assert_eq!(popover_height(900.0), 640.0);
        assert_eq!(popover_height(f64::NAN), 160.0, "a broken measurement never collapses the window");
    }
}
