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
        .inner_size(400.0, 600.0)
        .decorations(false)
        .transparent(true)
        .resizable(false)
        .visible(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .build()?;

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

/// Opens the history panel for an item, creating the window the first time.
pub fn open_panel(app: &AppHandle, item: &str) -> tauri::Result<()> {
    let url = format!("index.html?view=panel&item={item}");
    if let Some(w) = app.get_webview_window("panel") {
        w.eval(format!("window.location.replace('/{url}')"))?;
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "panel", WebviewUrl::App(url.into()))
        .title("History")
        .inner_size(560.0, 640.0)
        .min_inner_size(420.0, 360.0)
        .build()?;
    Ok(())
}
