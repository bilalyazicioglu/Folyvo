pub const APP_NAME: &str = "Folyvo";
pub mod api;
pub mod commands;
pub mod dto;
pub mod errors;
pub mod fit;
pub mod intent;
pub mod preview;
pub mod runtime;
#[cfg(target_os = "macos")]
pub mod services;
pub mod share;
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

/// Whether the app runs from an installed `.app` bundle, the only place its Finder extension exists.
pub fn runs_from_bundle(exe: &Path) -> bool {
    exe.ancestors().any(|p| p.extension().is_some_and(|e| e == "app"))
}

/// Turns on the Finder extension that ships inside Folyvo.app, so its items appear without a visit to
/// System Settings. macOS keeps the person's choice if they turn it off there later.
#[cfg(target_os = "macos")]
fn enable_finder_extension() {
    if std::env::current_exe().is_ok_and(|exe| runs_from_bundle(&exe)) {
        std::thread::spawn(|| {
            let _ = std::process::Command::new("/usr/bin/pluginkit")
                .args(["-e", "use", "-i", "dev.folyvo.app.findersync"])
                .status();
        });
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            commands::recent,
            commands::item,
            commands::history,
            commands::name_version,
            commands::export,
            commands::activity,
            commands::copies,
            commands::item_at,
            commands::which,
            commands::preview,
            commands::open_version,
            commands::restore,
            commands::stats,
            commands::folders,
            commands::add_folder,
            commands::remove_folder,
            commands::open_history,
            commands::hide_popover,
            commands::fit_popover,
            commands::set_dialog_open,
            commands::reveal,
            commands::take_which,
            commands::send_copy,
        ])
        .manage(tray::PendingWhich::default())
        .manage(intent::PendingIntent::default())
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
            #[cfg(target_os = "macos")]
            {
                services::register(app.handle());
                enable_finder_extension();
            }
            // A Finder or Services request that launched Folyvo is answered once the engine is up.
            if let Some(intent) = app.state::<intent::PendingIntent>().0.lock().unwrap().take() {
                intent::act_on(app.handle(), intent);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Folyvo could not start")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = event {
                let home = app.path().home_dir().unwrap_or_default();
                let allowed = |i: &intent::Intent| intent::from_link_allowed(&i.path, &home);
                if let Some(intent) = urls.iter().filter_map(|u| intent::parse_url(u.as_str())).find(allowed)
                {
                    intent::act_on(app, intent);
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_registers_the_folyvo_url_scheme() {
        let plist = include_str!("../Info.plist");
        let schemes = plist.split("<key>CFBundleURLSchemes</key>").nth(1).expect("a URL scheme is declared");
        assert!(schemes.trim_start().starts_with("<array><string>folyvo</string></array>"), "{plist}");
    }

    #[test]
    fn the_app_offers_four_services_named_for_folyvo() {
        let plist = include_str!("../Info.plist");
        let services = plist.split("<key>NSServices</key>").nth(1).expect("Services are declared");
        let entries: Vec<&str> = services.split("<key>NSMenuItem</key>").skip(1).collect();
        let titles = ["Browse All Versions…", "Send a Copy…", "Save a Copy…", "Which One Is This?…"];
        assert_eq!(entries.len(), titles.len());
        for (entry, title) in entries.iter().zip(titles) {
            assert!(entry.contains(&format!("<string>Folyvo: {title}</string>")), "{entry}");
            let message =
                entry.split("<key>NSMessage</key><string>").nth(1).unwrap().split('<').next().unwrap();
            assert!(intent::IntentKind::from_service_message(message).is_some(), "{message}");
            assert!(entry.contains("<key>NSPortName</key><string>Folyvo</string>"), "{entry}");
            assert!(
                entry.contains("<key>NSSendFileTypes</key><array><string>public.item</string></array>"),
                "{entry}"
            );
        }
    }

    #[test]
    fn only_a_bundled_app_enables_its_finder_extension() {
        assert!(runs_from_bundle(std::path::Path::new(
            "/Applications/Folyvo.app/Contents/MacOS/folyvo-desktop"
        )));
        assert!(!runs_from_bundle(std::path::Path::new("/Users/a/folyvo/target/debug/folyvo-desktop")));
    }

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
