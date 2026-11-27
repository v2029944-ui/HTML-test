//! Original UI management operations backed by revision-checked SQLite domains.
use crate::storage::{
    GroupTreeEdit, ManagementReceipt, UnifiedCatalogReceipt, UnifiedManagementPage, UnifiedSettingsReview,
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

#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GroupTreeAction {
    Create { id: String, name: String, parent: Option<String> },
    Move { group: String, parent: Option<String> },
    Delete { group: String, delete_descendants: bool, confirmed: bool, backup: Option<String> },
}

#[tauri::command]
pub async fn unified_group_tree_edit(
    app: AppHandle,
    revision: i64,
    action: GroupTreeAction,
    workspace_token: Option<String>,
) -> Result<ManagementReceipt, String> {
    fn required(value: String) -> Result<i64, String> {
        group_handle(Some(value))?.ok_or_else(|| "Выберите группу.".into())
    }
    let (edit, backup) = match action {
        GroupTreeAction::Create { id, name, parent } => (GroupTreeEdit::Create {
            id, name, parent: group_handle(parent)?,
        }, None),
        GroupTreeAction::Move { group, parent } => (GroupTreeEdit::Move {
            group: required(group)?, parent: group_handle(parent)?,
        }, None),
        GroupTreeAction::Delete { group, delete_descendants, confirmed, backup } => (GroupTreeEdit::Delete {
            group: required(group)?, delete_descendants, confirmed,
        }, backup),
    };
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app, workspace_token.as_deref(), crate::storage::shared_runtime::Access::Write,
            |store| store.unified_group_tree_edit_with_backup(
                revision, edit, backup.as_deref().map(std::path::Path::new),
            ),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn unified_groups_delete(
    app: AppHandle,
    revision: i64,
    handles: Option<Vec<String>>,
    confirmed_count: usize,
    backup: Option<String>,
    confirmed: bool,
    workspace_token: Option<String>,
) -> Result<ManagementReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let handles = handles.map(|values| {
            if values.len() > 10_000 {
                return Err("Выберите не более 10 000 групп.".into());
            }
            values.into_iter().map(|value| {
                group_handle(Some(value))?.ok_or_else(|| "Укажите группу.".to_string())
            }).collect::<Result<Vec<_>, String>>()
        }).transpose()?;
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Write,
            |store| store.unified_delete_groups(
                revision, handles.as_deref(), confirmed_count,
                backup.as_deref().map(std::path::Path::new), confirmed,
            ),
        )
    })
    .await
    .map_err(|error| error.to_string())?
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
    backup: Option<String>,
    workspace_token: Option<String>,
) -> Result<ManagementReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Write,
            |routed_storage| {
                routed_storage.unified_groups_import_with_backup(
                    revision,
                    std::path::Path::new(&source),
                    &hash,
                    confirmed,
                    backup.as_deref().map(std::path::Path::new),
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
