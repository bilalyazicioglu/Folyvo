use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::image::Image;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

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
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(true)
        .tooltip("Folyvo")
        .on_tray_icon_event(move |tray, event| {
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
                    let _ = show_popover(app);
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
/// The label of the window that shows one file's saved times.
pub const VERSIONS: &str = "versions";

/// The label of the window that lists every kept file.
pub const ALL_FILES: &str = "files";

/// The label of the window shown on first launch.
pub const FIRST_WINDOW: &str = "first";

const TRAY_ID: &str = "folyvo";

/// Logical points between the popover and the menu bar.
const POPOVER_GAP: f64 = 6.0;

/// Whether Folyvo should be in the Dock and in Command-Tab: while one of its windows is open, so a
/// person can switch back to it like to any app. Otherwise it lives in the menu bar only.
pub fn in_dock<'a>(open: impl IntoIterator<Item = &'a str>, closing: Option<&str>) -> bool {
    open.into_iter()
        .any(|label| [VERSIONS, ALL_FILES, FIRST_WINDOW].contains(&label) && closing != Some(label))
}

/// Shows or hides Folyvo in the Dock and Command-Tab to match the windows open now.
pub fn follow_windows(app: &AppHandle, closing: Option<&str>) {
    #[cfg(target_os = "macos")]
    {
        let windows = app.webview_windows();
        let policy = if in_dock(windows.keys().map(String::as_str), closing) {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        let _ = app.set_activation_policy(policy);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, closing);
}

pub fn open_versions(app: &AppHandle, item: &str, action: Option<&str>) -> tauri::Result<()> {
    let action = action.map(|a| format!("&action={a}")).unwrap_or_default();
    let url = format!("index.html?view=versions&item={item}{action}");
    if let Some(w) = app.get_webview_window(VERSIONS) {
        w.eval(format!("window.location.replace('/{url}')"))?;
        w.show()?;
        w.set_focus()?;
        follow_windows(app, None);
        return Ok(());
    }
    let builder = WebviewWindowBuilder::new(app, VERSIONS, WebviewUrl::App(url.into()))
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
    follow_windows(app, None);
    window.set_focus()?;
    Ok(())
}

/// Opens the window that lists every kept file, or brings it back.
pub fn open_all_files(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(ALL_FILES) {
        w.show()?;
        w.set_focus()?;
        follow_windows(app, None);
        return Ok(());
    }
    let builder = WebviewWindowBuilder::new(app, ALL_FILES, WebviewUrl::App("index.html?view=files".into()))
        .title("All Files")
        .inner_size(520.0, 640.0)
        .min_inner_size(400.0, 320.0);
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(22.0, 28.0))
        .transparent(true);
    let window = builder.build()?;
    window_glass(&window);
    #[cfg(target_os = "macos")]
    {
        let size = window.inner_size()?;
        window.set_size(tauri::PhysicalSize::new(size.width + 1, size.height))?;
        window.set_size(size)?;
    }
    follow_windows(app, None);
    window.set_focus()
}

/// The window a new person sees first. It is a fixed size: there is nothing in it to make room for.
pub fn open_first_window(app: &AppHandle) -> tauri::Result<()> {
    let builder =
        WebviewWindowBuilder::new(app, FIRST_WINDOW, WebviewUrl::App("index.html?view=first".into()))
            .title("Welcome to Folyvo")
            .inner_size(460.0, 640.0)
            .resizable(false)
            .center();
    #[cfg(target_os = "macos")]
    let builder = builder.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true);
    let window = builder.build()?;
    follow_windows(app, None);
    window.set_focus()
}

/// Closes the first window. The popover follows from `after_first_window`, however it was closed.
pub fn finish_first_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(FIRST_WINDOW) {
        w.close()?;
    }
    Ok(())
}

/// Opens the popover, which is where Folyvo lives from now on. Leaving the Dock takes focus from the
/// app a moment after the window has gone, and a popover without focus hides, so it waits for that.
pub fn after_first_window(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(250));
        let main = app.clone();
        let _ = app.run_on_main_thread(move || {
            let _ = show_popover(&main);
        });
    });
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
    show_popover(app)?;
    app.emit_to("popover", "intent-which", ())
}

/// Shows the popover under the menu bar icon.
fn show_popover(app: &AppHandle) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("popover") else { return Ok(()) };
    // Before the menu bar icon was ever clicked its position is unknown; the top right is where it lives.
    if let Some((x, y)) = popover_position(app) {
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    }
    window.show()?;
    window.set_focus()
}

fn popover_position(app: &AppHandle) -> Option<(i32, i32)> {
    let icon = app.tray_by_id(TRAY_ID)?.rect().ok().flatten().map(|rect| {
        let (at, size) = (rect.position.to_physical::<i32>(1.0), rect.size.to_physical::<u32>(1.0));
        ((at.x, at.y), (size.width, size.height))
    });
    let monitor = match icon {
        Some(((x, y), _)) => app.monitor_from_point(f64::from(x), f64::from(y)).ok().flatten(),
        None => None,
    }
    .or_else(|| app.primary_monitor().ok().flatten())?;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    Some(popover_origin(
        icon,
        ((area.position.x, area.position.y), (area.size.width, area.size.height)),
        (crate::fit::POPOVER_WIDTH * scale).round() as i32,
        (POPOVER_GAP * scale).round() as i32,
    ))
}

/// Centred under the icon with a little air, and never past the side of the screen. Without an
/// icon position it sits in the top right, where menu bar icons live.
fn popover_origin(
    icon: Option<((i32, i32), (u32, u32))>,
    ((area_x, area_y), (area_width, _)): ((i32, i32), (u32, u32)),
    width: i32,
    gap: i32,
) -> (i32, i32) {
    let right_most = area_x + area_width as i32 - width - gap;
    match icon {
        Some(((x, y), (icon_width, icon_height))) => {
            let centred = x + icon_width as i32 / 2 - width / 2;
            (centred.clamp(area_x + gap, right_most), y + icon_height as i32 + gap)
        }
        None => (right_most, area_y + gap),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: ((i32, i32), (u32, u32)) = ((0, 74), (3024, 1890));

    #[test]
    fn the_popover_opens_centred_under_the_icon_with_a_gap() {
        assert_eq!(popover_origin(Some(((1000, 0), (60, 74))), AREA, 760, 12), (650, 86));
    }

    #[test]
    fn the_popover_stays_on_the_screen_when_the_icon_is_near_a_side() {
        assert_eq!(popover_origin(Some(((2960, 0), (60, 74))), AREA, 760, 12).0, 2252);
        assert_eq!(popover_origin(Some(((-400, 0), (60, 74))), AREA, 760, 12).0, 12);
    }

    #[test]
    fn without_an_icon_position_the_popover_sits_in_the_top_right() {
        assert_eq!(popover_origin(None, AREA, 760, 12), (2252, 86));
    }

    #[test]
    fn folyvo_is_in_the_dock_only_while_one_of_its_windows_is_open() {
        assert!(!in_dock(["popover"], None));
        assert!(in_dock(["popover", VERSIONS], None));
        assert!(in_dock(["popover", ALL_FILES], None));
        assert!(in_dock(["popover", FIRST_WINDOW], None));
        assert!(!in_dock(["popover", VERSIONS], Some(VERSIONS)), "the window being closed no longer counts");
        assert!(in_dock([VERSIONS, ALL_FILES], Some(VERSIONS)), "the other one is still open");
    }
}
