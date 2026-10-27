use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::image::Image;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_positioner::{Position, WindowExt};

use crate::dto::EngineEventDto;
use crate::runtime::Notifier;
use crate::toggle::{ToggleGate, TrayAction};

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Forwards engine events to every webview.
pub struct TauriNotifier(pub AppHandle);

impl Notifier for TauriNotifier {
    fn notify(&self, event: EngineEventDto) {
        let _ = self.0.emit("engine-event", event);
    }
}

pub fn build(app: &tauri::App) -> tauri::Result<()> {
    let popover = WebviewWindowBuilder::new(app, "popover", WebviewUrl::App("index.html".into()))
        .title("Folyvo")
        .inner_size(crate::fit::POPOVER_WIDTH, 560.0)
        .decorations(false)
        .transparent(true)
        .resizable(false)
        .visible(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .build()?;

    glass(&popover);

    let gate = std::sync::Arc::new(Mutex::new(ToggleGate::new(300)));
    app.manage(gate.clone());
    let blur_gate = gate.clone();
    let hider = popover.clone();
    popover.on_window_event(move |event| {
        if let WindowEvent::Focused(false) = event {
            if blur_gate.lock().unwrap().on_blur(now_ms()) {
                let _ = hider.hide();
            }
        }
    });

    let icon = Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    TrayIconBuilder::with_id("folyvo")
        .icon(icon)
        .icon_as_template(true)
        .tooltip("Folyvo")
        .on_tray_icon_event(move |tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
            let TrayIconEvent::Click {
                button: MouseButton::Left, button_state: MouseButtonState::Up, ..
            } = event
            else {
                return;
            };
            let app = tray.app_handle();
            let Some(window) = app.get_webview_window("popover") else { return };
            let visible = window.is_visible().unwrap_or(false);
            match gate.lock().unwrap().on_tray_click(now_ms(), visible) {
                TrayAction::Show => {
                    let _ = window.move_window(Position::TrayBottomCenter);
                    let _ = window.show();
                    let _ = window.set_focus();
                    let _ = app.emit("engine-event", EngineEventDto::Resync);
                }
                TrayAction::Hide => {
                    let _ = window.hide();
                }
                TrayAction::Ignore => {}
            }
        })
        .build(app)?;
    Ok(())
}

/// Liquid Glass behind the popover on macOS 26 and newer, the classic popover material before.
/// Either way macOS draws it opaque when Reduce Transparency is on.
#[cfg(target_os = "macos")]
fn glass(window: &tauri::WebviewWindow) {
    use window_vibrancy::{
        apply_liquid_glass, apply_vibrancy, LiquidGlassOptions, NSGlassEffectViewStyle,
        NSVisualEffectMaterial, NSVisualEffectState,
    };
    let liquid = LiquidGlassOptions::new(NSGlassEffectViewStyle::Regular).radius(24.0);
    if apply_liquid_glass(window, liquid).is_err() {
        let _ = apply_vibrancy(
            window,
            NSVisualEffectMaterial::Popover,
            Some(NSVisualEffectState::Active),
            Some(24.0),
        );
    }
}

#[cfg(not(target_os = "macos"))]
fn glass(_window: &tauri::WebviewWindow) {}

/// Opens the versions window for an item, creating it the first time. `action` (`send` or `save`)
/// asks the window to start Send a Copy or Save a Copy for the newest save once it has loaded.
pub fn open_versions(app: &AppHandle, item: &str, action: Option<&str>) -> tauri::Result<()> {
    let action = action.map(|a| format!("&action={a}")).unwrap_or_default();
    let url = format!("index.html?view=versions&item={item}{action}");
    if let Some(w) = app.get_webview_window("versions") {
        w.eval(format!("window.location.replace('/{url}')"))?;
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }
    let builder = WebviewWindowBuilder::new(app, "versions", WebviewUrl::App(url.into()))
        .title("Versions")
        .inner_size(1100.0, 760.0)
        .min_inner_size(760.0, 520.0);
    // The window controls sit on the page, and the sidebar is see-through to the system material.
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(22.0, 28.0))
        .transparent(true);
    let window = builder.build()?;
    window_glass(&window);
    // Installing the glass lays the title bar out again and puts the window controls back in the
    // corner; a resize makes the window apply their position once more.
    #[cfg(target_os = "macos")]
    {
        let size = window.inner_size()?;
        window.set_size(tauri::PhysicalSize::new(size.width + 1, size.height))?;
        window.set_size(size)?;
    }
    Ok(())
}

/// Liquid Glass behind the whole versions window; the page lays the stage over it as an opaque panel
/// and leaves the saved times on the glass. Falls back to the sidebar material before macOS 26.
#[cfg(target_os = "macos")]
fn window_glass(window: &tauri::WebviewWindow) {
    use window_vibrancy::{
        apply_liquid_glass, apply_vibrancy, LiquidGlassOptions, NSGlassEffectViewStyle,
        NSVisualEffectMaterial, NSVisualEffectState,
    };
    if apply_liquid_glass(window, LiquidGlassOptions::new(NSGlassEffectViewStyle::Regular)).is_err() {
        let _ = apply_vibrancy(
            window,
            NSVisualEffectMaterial::Sidebar,
            Some(NSVisualEffectState::FollowsWindowActiveState),
            None,
        );
    }
}

#[cfg(not(target_os = "macos"))]
fn window_glass(_window: &tauri::WebviewWindow) {}

/// A file someone asked "which one is this?" about, waiting for the popover to take it.
#[derive(Default)]
pub struct PendingWhich(pub Mutex<Option<String>>);

/// Shows the popover under the menu bar icon and has it answer "which one is this?" for `path`.
pub fn show_which(app: &AppHandle, path: &std::path::Path) -> tauri::Result<()> {
    if let Some(pending) = app.try_state::<PendingWhich>() {
        *pending.0.lock().unwrap() = Some(path.to_string_lossy().into_owned());
    }
    let Some(window) = app.get_webview_window("popover") else { return Ok(()) };
    // Before the menu bar icon was ever clicked its position is unknown; the top right is where it lives.
    if window.move_window(Position::TrayBottomCenter).is_err() {
        let _ = window.move_window(Position::TopRight);
    }
    window.show()?;
    window.set_focus()?;
    app.emit_to("popover", "intent-which", ())
}
