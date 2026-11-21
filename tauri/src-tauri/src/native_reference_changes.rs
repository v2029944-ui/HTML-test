//! IPC adapters for explicit independent reference comparison intervals.
use crate::storage::{ReferenceChangesPage, ReferenceChangesRequest};
use tauri::AppHandle;

#[tauri::command]
pub async fn reference_changes(
    app: AppHandle,
    request: ReferenceChangesRequest,
    workspace_token: Option<String>,
) -> Result<ReferenceChangesPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| routed_storage.reference_changes(&request),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}
#[tauri::command]
pub async fn reference_changes_csv(
    app: AppHandle,
    request: ReferenceChangesRequest,
    destination: String,
    workspace_token: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                routed_storage.reference_changes_csv(&request, std::path::Path::new(&destination))
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn reference_summary_csv(
    app: AppHandle,
    request: ReferenceChangesRequest,
    destination: String,
    workspace_token: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                routed_storage.reference_summary_csv(&request, std::path::Path::new(&destination))
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
}
