pub const APP_NAME: &str = "Folyvo";
pub mod api;
pub mod commands;
pub mod dto;
pub mod errors;
pub mod runtime;
pub mod toggle;
pub mod tray;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use folyvo_core::ports::{RandomIdGen, SystemClock};
use folyvo_core::runner::DEFAULT_QUIET;
use tauri::Manager;

pub fn vault_dir(env_override: Option<OsString>, data_dir: &Path) -> PathBuf {
    match env_override {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => data_dir.join("Folyvo"),
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::recent,
            commands::history,
            commands::name_version,
            commands::export,
            commands::activity,
            commands::copies,
            commands::item_at,
            commands::which,
            commands::restore,
            commands::stats,
            commands::folders,
            commands::add_folder,
            commands::remove_folder,
            commands::open_history,
            commands::hide_popover,
            commands::set_dialog_open,
            commands::reveal,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let vault = vault_dir(std::env::var_os("FOLYVO_VAULT"), &app.path().data_dir()?);
            let notifier = Arc::new(tray::TauriNotifier(app.handle().clone()));
            let runtime = runtime::AppRuntime::start(
                &vault,
                Arc::new(SystemClock),
                Arc::new(RandomIdGen),
                DEFAULT_QUIET,
                notifier,
            )
            .map_err(|e| e.to_string())?;
            app.manage(runtime);
            tray::build(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Folyvo could not start");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_is_named_folyvo() {
        assert_eq!(APP_NAME, "Folyvo");
    }

    #[test]
    fn the_vault_lives_in_application_support_unless_overridden() {
        let data = std::path::Path::new("/Users/a/Library/Application Support");
        assert_eq!(vault_dir(None, data), data.join("Folyvo"));
        assert_eq!(
            vault_dir(Some("/tmp/demo-vault".into()), data),
            std::path::PathBuf::from("/tmp/demo-vault")
        );
        assert_eq!(vault_dir(Some("".into()), data), data.join("Folyvo"), "an empty override is ignored");
    }
}
