//! Starting with the Mac: Folyvo keeps saves only while it runs, so it adds itself to the login items
//! once, the first time an installed copy starts. After that the choice is the person's, in System
//! Settings, and Folyvo never adds itself back.

use std::path::Path;

/// The file in the vault that says Folyvo has already added itself once.
const ASKED: &str = "login-item-added";

/// What this start does about the login items.
#[derive(Debug, PartialEq, Eq)]
pub enum FirstStart {
    /// Leave everything alone: a development build, or the choice is the person's already.
    Nothing,
    /// It is on already: remember that, so turning it off later stays off.
    Remember,
    Add,
}

pub fn first_start(in_bundle: bool, added_before: bool, already_on: bool) -> FirstStart {
    match (in_bundle && !added_before, already_on) {
        (false, _) => FirstStart::Nothing,
        (true, true) => FirstStart::Remember,
        (true, false) => FirstStart::Add,
    }
}

pub fn added_before(vault: &Path) -> bool {
    vault.join(ASKED).exists()
}

pub fn remember_added(vault: &Path) -> std::io::Result<()> {
    std::fs::write(vault.join(ASKED), b"")
}

/// Adds Folyvo to the login items if this is the first start of an installed copy.
#[cfg(target_os = "macos")]
pub fn add_once(vault: &Path) {
    use objc2_service_management::{SMAppService, SMAppServiceStatus};
    let in_bundle = std::env::current_exe().is_ok_and(|exe| crate::runs_from_bundle(&exe));
    if !in_bundle {
        return;
    }
    // SAFETY: plain calls on the service for this app; nothing here outlives them.
    let service = unsafe { SMAppService::mainAppService() };
    let already_on = unsafe { service.status() } == SMAppServiceStatus::Enabled;
    match first_start(in_bundle, added_before(vault), already_on) {
        FirstStart::Nothing => return,
        FirstStart::Remember => {}
        FirstStart::Add => {
            if let Err(e) = unsafe { service.registerAndReturnError() } {
                eprintln!("folyvo: could not add the login item: {e}");
                return;
            }
        }
    }
    if let Err(e) = remember_added(vault) {
        eprintln!("folyvo: could not remember the login item: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folyvo_adds_itself_only_once_and_only_when_installed() {
        assert_eq!(first_start(true, false, false), FirstStart::Add, "first start of an installed copy");
        assert_eq!(first_start(false, false, false), FirstStart::Nothing, "a development build stays out");
        assert_eq!(first_start(true, true, false), FirstStart::Nothing, "the person may have turned it off");
    }

    #[test]
    fn a_login_item_already_on_is_remembered_so_turning_it_off_later_lasts() {
        assert_eq!(first_start(true, false, true), FirstStart::Remember);
        assert_eq!(first_start(true, true, true), FirstStart::Nothing);
    }

    #[test]
    fn having_added_itself_is_remembered_in_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!added_before(dir.path()));
        remember_added(dir.path()).unwrap();
        assert!(added_before(dir.path()));
    }
}
