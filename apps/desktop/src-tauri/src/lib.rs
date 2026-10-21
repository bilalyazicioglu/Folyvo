pub const APP_NAME: &str = "Folyvo";

/// Starts the app. Filled in by the tray task.
pub fn run() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_is_named_folyvo() {
        assert_eq!(APP_NAME, "Folyvo");
    }
}
