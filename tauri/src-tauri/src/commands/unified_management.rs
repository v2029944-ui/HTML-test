//! Original UI management operations backed by revision-checked SQLite domains.
use crate::storage::{
    ManagementReceipt, UnifiedCatalogReceipt, UnifiedManagementPage, UnifiedSettingsReview,
};
use tauri::AppHandle;

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
    workspace_token: Option<String>,
) -> Result<UnifiedManagementPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                routed_storage.unified_management_page(
                    revision,
                    &kind,
                    &query,
                    group_handle(group)?,
                    offset,
                    limit,
                )
            },
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
    workspace_token: Option<String>,
) -> Result<ManagementReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Write,
            |routed_storage| {
                routed_storage.unified_save_group(
                    revision,
                    group_handle(handle)?,
                    &id,
                    &name,
                    &works,
                )
            },
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
    workspace_token: Option<String>,
) -> Result<ManagementReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Write,
            |routed_storage| {
                routed_storage.unified_assign_work(revision, &work, group_handle(group)?)
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_groups_preview(
    app: AppHandle,
    revision: i64,
    source: String,
    workspace_token: Option<String>,
) -> Result<UnifiedSettingsReview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                routed_storage.unified_groups_preview(revision, std::path::Path::new(&source))
            },
        )
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
    workspace_token: Option<String>,
) -> Result<ManagementReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Write,
            |routed_storage| {
                routed_storage.unified_groups_import(
                    revision,
                    std::path::Path::new(&source),
                    &hash,
                    confirmed,
                )
            },
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
    workspace_token: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                routed_storage.unified_groups_export(revision, std::path::Path::new(&destination))
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_catalog_preview(
    app: AppHandle,
    revision: i64,
    source: String,
    workspace_token: Option<String>,
) -> Result<UnifiedSettingsReview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                routed_storage.unified_catalog_preview(revision, std::path::Path::new(&source))
            },
        )
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
    workspace_token: Option<String>,
) -> Result<UnifiedCatalogReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Write,
            |routed_storage| {
                routed_storage.unified_catalog_apply(
                    revision,
                    std::path::Path::new(&source),
                    &hash,
                    confirmed,
                )
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
