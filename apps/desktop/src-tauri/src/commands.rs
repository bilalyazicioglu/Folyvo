use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use crate::dto::{
    ActivityDto, CopyDto, ExportDto, FolderDto, ItemSummaryDto, MarkedPageDto, MatchDto, PreviewDto,
    RestoreDto, VaultStatsDto, VersionDto,
};
use crate::errors::UserError;
use crate::runtime::AppRuntime;

type Rt<'a> = State<'a, Arc<AppRuntime>>;

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, UserError> + Send + 'static,
) -> Result<T, UserError> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|_| UserError::internal())?
}

#[tauri::command]
pub async fn recent(rt: Rt<'_>, limit: u32) -> Result<Vec<ItemSummaryDto>, UserError> {
    let api = rt.api();
    blocking(move || api.recent(limit)).await
}

#[tauri::command]
pub async fn item(rt: Rt<'_>, item: String) -> Result<ItemSummaryDto, UserError> {
    let api = rt.api();
    blocking(move || api.item(&item)).await
}

#[tauri::command]
pub async fn history(rt: Rt<'_>, item: String) -> Result<Vec<VersionDto>, UserError> {
    let api = rt.api();
    let runtime = rt.inner().clone();
    blocking(move || {
        let history = api.history(&item)?;
        runtime.backfill(&item);
        Ok(history)
    })
    .await
}

#[tauri::command]
pub async fn name_version(rt: Rt<'_>, version: String, note: Option<String>) -> Result<(), UserError> {
    let api = rt.api();
    blocking(move || api.name_version(&version, note)).await
}

#[tauri::command]
pub async fn export(
    rt: Rt<'_>,
    item: String,
    version: String,
    dest_dir: String,
) -> Result<ExportDto, UserError> {
    let api = rt.api();
    blocking(move || api.export(&item, &version, &dest_dir)).await
}

#[tauri::command]
pub async fn activity(rt: Rt<'_>, from: i64, to: i64) -> Result<ActivityDto, UserError> {
    let api = rt.api();
    blocking(move || api.activity(from, to)).await
}

#[tauri::command]
pub async fn copies(rt: Rt<'_>, item: String) -> Result<Vec<CopyDto>, UserError> {
    let api = rt.api();
    blocking(move || api.copies(&item)).await
}

#[tauri::command]
pub async fn item_at(rt: Rt<'_>, path: String) -> Result<Option<String>, UserError> {
    let api = rt.api();
    blocking(move || api.item_at(&path)).await
}

#[tauri::command]
pub async fn preview(rt: Rt<'_>, item: String, version: String) -> Result<PreviewDto, UserError> {
    let api = rt.api();
    blocking(move || api.preview(&item, &version)).await
}

#[tauri::command]
pub async fn changes(rt: Rt<'_>, item: String, version: String) -> Result<MarkedPageDto, UserError> {
    let api = rt.api();
    blocking(move || api.changes(&item, &version)).await
}

#[tauri::command]
pub async fn open_version(rt: Rt<'_>, item: String, version: String, newest: bool) -> Result<(), UserError> {
    let api = rt.api();
    blocking(move || api.open_version(&item, &version, newest)).await
}

/// Shows the share sheet for a read-only copy of `version`, next to the button at `x, y, w, h` in the
/// calling window. A service that takes the file is recorded as a handed copy.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn send_copy(
    app: AppHandle,
    window: tauri::WebviewWindow,
    rt: Rt<'_>,
    item: String,
    version: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<(), UserError> {
    let api = rt.api();
    let (i, v) = (item.clone(), version.clone());
    let file = blocking(move || api.share_file(&i, &v)).await?;
    show_share(app, window, item, version, file, (x, y, w, h))
}

#[cfg(target_os = "macos")]
fn show_share(
    app: AppHandle,
    window: tauri::WebviewWindow,
    item: String,
    version: String,
    file: PathBuf,
    (x, y, w, h): (f64, f64, f64, f64),
) -> Result<(), UserError> {
    window
        .with_webview(move |wv| {
            use objc2::MainThreadMarker;
            use objc2_app_kit::NSView;
            use objc2_foundation::{NSPoint, NSRect, NSSize};
            let Some(mtm) = MainThreadMarker::new() else { return };
            // SAFETY: with_webview runs on the main thread with the window's live WKWebView, an NSView.
            let view: &NSView = unsafe { &*(wv.inner() as *const NSView) };
            let y = if view.isFlipped() { y } else { view.bounds().size.height - y - h };
            let rect = NSRect::new(NSPoint::new(x, y), NSSize::new(w, h));
            crate::share::mac::show(mtm, view, rect, &file, move |service| {
                if let Some(rt) = app.try_state::<Arc<AppRuntime>>() {
                    let _ = rt.api().record_share(&item, &version, &service);
                }
            });
        })
        .map_err(|_| UserError::internal())
}

#[cfg(not(target_os = "macos"))]
fn show_share(
    _app: AppHandle,
    _window: tauri::WebviewWindow,
    _item: String,
    _version: String,
    _file: PathBuf,
    _rect: (f64, f64, f64, f64),
) -> Result<(), UserError> {
    Err(UserError::new(
        "not_supported",
        "Send a Copy needs macOS",
        "Use Save a Copy… to copy the file out instead.",
    ))
}

#[tauri::command]
pub async fn which(rt: Rt<'_>, path: String) -> Result<Vec<MatchDto>, UserError> {
    let api = rt.api();
    blocking(move || api.which(&path)).await
}

#[tauri::command]
pub async fn restore(rt: Rt<'_>, item: String, version: String) -> Result<RestoreDto, UserError> {
    let api = rt.api();
    blocking(move || api.restore(&item, &version)).await
}

#[tauri::command]
pub async fn stats(rt: Rt<'_>) -> Result<VaultStatsDto, UserError> {
    let api = rt.api();
    blocking(move || api.stats()).await
}

#[tauri::command]
pub async fn folders(rt: Rt<'_>) -> Result<Vec<FolderDto>, UserError> {
    let rt = rt.inner().clone();
    blocking(move || Ok(rt.folders())).await
}

#[tauri::command]
pub async fn add_folder(rt: Rt<'_>, path: String) -> Result<(), UserError> {
    let rt = rt.inner().clone();
    blocking(move || rt.add_folder(&PathBuf::from(path))).await
}

#[tauri::command]
pub async fn remove_folder(rt: Rt<'_>, path: String) -> Result<(), UserError> {
    let rt = rt.inner().clone();
    blocking(move || rt.remove_folder(&PathBuf::from(path))).await
}

#[tauri::command]
pub fn open_history(app: AppHandle, item: String) -> Result<(), UserError> {
    folyvo_core::model::ItemId::parse(&item).map_err(UserError::from)?;
    crate::tray::open_versions(&app, &item, None).map_err(|_| UserError::internal())
}

/// The popover asks for the height of its content; the window follows within limits.
#[tauri::command]
pub fn fit_popover(app: AppHandle, height: f64) {
    if let Some(w) = app.get_webview_window("popover") {
        let size = tauri::LogicalSize::new(crate::fit::POPOVER_WIDTH, crate::fit::popover_height(height));
        let _ = w.set_size(size);
    }
}

/// The UI tells the shell while a native dialog is open, so losing focus does not hide the popover.
#[tauri::command]
pub fn set_dialog_open(
    gate: tauri::State<'_, std::sync::Arc<std::sync::Mutex<crate::toggle::ToggleGate>>>,
    open: bool,
) {
    gate.lock().unwrap().set_dialog_open(open);
}

#[tauri::command]
pub fn hide_popover(app: AppHandle) {
    if let Some(w) = app.get_webview_window("popover") {
        let _ = w.hide();
    }
}

/// Shows a file in Finder. Only paths that exist are accepted, and the path is passed as one
/// argument, never through a shell.
#[tauri::command]
pub fn reveal(path: String) -> Result<(), UserError> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(UserError::new(
            "file_unavailable",
            "Folyvo can't reach that file",
            "Check that it still exists.",
        ));
    }
    std::process::Command::new("open").arg("-R").arg(&p).status().map_err(|_| UserError::internal())?;
    Ok(())
}

/// The file waiting for a "which one is this?" answer, taken so it is answered once.
#[tauri::command]
pub fn take_which(pending: State<'_, crate::tray::PendingWhich>) -> Option<String> {
    pending.0.lock().unwrap().take()
}
