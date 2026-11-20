//! IPC adapters for explicit independent reference comparison intervals.
use crate::storage::{ReferenceChangesPage, ReferenceChangesRequest, Storage};
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn reference_changes(
    app: AppHandle,
    request: ReferenceChangesRequest,
) -> Result<ReferenceChangesPage, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Storage>().reference_changes(&request))
        .await
        .map_err(|error| error.to_string())?
}
#[tauri::command]
pub async fn reference_changes_csv(
    app: AppHandle,
    request: ReferenceChangesRequest,
    destination: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>()
            .reference_changes_csv(&request, std::path::Path::new(&destination))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn reference_summary_csv(
    app: AppHandle,
    request: ReferenceChangesRequest,
    destination: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>()
            .reference_summary_csv(&request, std::path::Path::new(&destination))
    })
    .await
    .map_err(|error| error.to_string())?
}
