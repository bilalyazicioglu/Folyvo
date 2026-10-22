use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use crate::dto::{ExportDto, FolderDto, ItemSummaryDto, RestoreDto, VaultStatsDto, VersionDto};
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
pub async fn history(rt: Rt<'_>, item: String) -> Result<Vec<VersionDto>, UserError> {
    let api = rt.api();
    blocking(move || api.history(&item)).await
}

#[tauri::command]
pub async fn name_version(rt: Rt<'_>, version: String, note: Option<String>) -> Result<(), UserError> {
    let api = rt.api();
    blocking(move || api.name_version(&version, note)).await
}

#[tauri::command]
pub async fn mark_ready(rt: Rt<'_>, item: String, version: String) -> Result<(), UserError> {
    let api = rt.api();
    blocking(move || api.mark_ready(&item, &version)).await
}

#[tauri::command]
pub async fn export(
    rt: Rt<'_>,
    item: String,
    expected: String,
    dest_dir: String,
) -> Result<ExportDto, UserError> {
    let api = rt.api();
    blocking(move || api.export(&item, &expected, &dest_dir)).await
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
    crate::tray::open_panel(&app, &item).map_err(|_| UserError::internal())
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
