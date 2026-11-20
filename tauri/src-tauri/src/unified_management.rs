//! Original UI management operations backed by revision-checked SQLite domains.
use crate::storage::{
    ManagementReceipt, Storage, UnifiedCatalogReceipt, UnifiedManagementPage, UnifiedSettingsReview,
};
use tauri::{AppHandle, Manager};

fn group_handle(value: Option<String>) -> Result<Option<i64>, String> {
    value
        .map(|s| {
            s.parse::<i64>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| "Недопустимый идентификатор группы.".to_string())
        })
        .transpose()
}

#[tauri::command]
pub async fn unified_management_page(
    app: AppHandle,
    revision: i64,
    kind: String,
    query: String,
    group: Option<String>,
    offset: u32,
    limit: usize,
) -> Result<UnifiedManagementPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().unified_management_page(
            revision,
            &kind,
            &query,
            group_handle(group)?,
            offset,
            limit,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_group_save(
    app: AppHandle,
    revision: i64,
    handle: Option<String>,
    id: String,
    name: String,
    works: Vec<String>,
) -> Result<ManagementReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().unified_save_group(
            revision,
            group_handle(handle)?,
            &id,
            &name,
            &works,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_work_assign(
    app: AppHandle,
    revision: i64,
    work: String,
    group: Option<String>,
) -> Result<ManagementReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>()
            .unified_assign_work(revision, &work, group_handle(group)?)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_groups_preview(
    app: AppHandle,
    revision: i64,
    source: String,
) -> Result<UnifiedSettingsReview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>()
            .unified_groups_preview(revision, std::path::Path::new(&source))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn unified_groups_import(
    app: AppHandle,
    revision: i64,
    source: String,
    hash: String,
    confirmed: bool,
) -> Result<ManagementReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().unified_groups_import(
            revision,
            std::path::Path::new(&source),
            &hash,
            confirmed,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn unified_groups_export(
    app: AppHandle,
    revision: i64,
    destination: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>()
            .unified_groups_export(revision, std::path::Path::new(&destination))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_catalog_preview(
    app: AppHandle,
    revision: i64,
    source: String,
) -> Result<UnifiedSettingsReview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>()
            .unified_catalog_preview(revision, std::path::Path::new(&source))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn unified_catalog_apply(
    app: AppHandle,
    revision: i64,
    source: String,
    hash: String,
    confirmed: bool,
) -> Result<UnifiedCatalogReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().unified_catalog_apply(
            revision,
            std::path::Path::new(&source),
            &hash,
            confirmed,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
