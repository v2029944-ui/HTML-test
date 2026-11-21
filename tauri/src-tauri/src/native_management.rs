//! Revision-checked settings pages and one-edit transactional publication for the SQL workspace.
use crate::storage::{ManagementJob, ManagementReceipt, Storage};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::AppHandle;

const PAGE_LIMIT: usize = 50;
const STAGE_QUOTA: u64 = 256 * 1024 * 1024;

#[derive(Serialize)]
pub struct NativeMember {
    handle: String,
    work: String,
}

#[derive(Serialize)]
pub struct NativeCatalogEntry {
    handle: String,
    work: String,
    organization: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SettingsAction {
    CreateGroup {
        id: String,
        name: String,
    },
    DeleteGroup {
        handle: String,
    },
    AppendMember {
        group: String,
        work: String,
    },
    RemoveMember {
        group: String,
        handle: String,
    },
    CreateCatalog {
        work: String,
        organization: Option<String>,
    },
    UpdateCatalog {
        handle: String,
        work: String,
        organization: Option<String>,
    },
    DeleteCatalog {
        handle: String,
    },
}

#[derive(Serialize)]
pub struct NativeSettingsReceipt {
    revision: i64,
    cleanup_warning: bool,
}

fn handle(value: &str) -> Result<i64, String> {
    value
        .parse::<i64>()
        .ok()
        .filter(|number| *number > 0)
        .ok_or_else(|| "Недопустимый идентификатор записи.".to_string())
}

pub(super) fn page_limit(limit: usize) -> Result<(), String> {
    if !(1..=PAGE_LIMIT).contains(&limit) {
        return Err("Размер страницы должен быть от 1 до 50 записей.".into());
    }
    Ok(())
}

fn require_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Введите название.".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn native_member_page(
    app: AppHandle,
    revision: i64,
    group: String,
    after_handle: Option<String>,
    limit: usize,
    workspace_token: Option<String>,
) -> Result<Vec<NativeMember>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                page_limit(limit)?;
                let after = after_handle.as_deref().map(handle).transpose()?;
                let rows = routed_storage.management_member_page(
                    revision,
                    handle(&group)?,
                    after,
                    limit,
                )?;
                Ok(rows
                    .into_iter()
                    .map(|row| NativeMember {
                        handle: row.handle.to_string(),
                        work: row.work,
                    })
                    .collect())
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn native_catalog_page(
    app: AppHandle,
    revision: i64,
    after_handle: Option<String>,
    limit: usize,
    workspace_token: Option<String>,
) -> Result<Vec<NativeCatalogEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                page_limit(limit)?;
                let after = after_handle.as_deref().map(handle).transpose()?;
                let rows =
                    routed_storage.management_catalog_page(revision, "", None, after, limit)?;
                Ok(rows
                    .into_iter()
                    .map(|row| NativeCatalogEntry {
                        handle: row.handle.to_string(),
                        work: row.work,
                        organization: row.organization,
                    })
                    .collect())
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn native_settings_edit(
    app: AppHandle,
    revision: i64,
    action: SettingsAction,
    workspace_token: Option<String>,
) -> Result<NativeSettingsReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Write,
            |routed_storage| publish(&routed_storage, revision, action),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

fn publish(
    store: &Storage,
    revision: i64,
    action: SettingsAction,
) -> Result<NativeSettingsReceipt, String> {
    let mut editor =
        store.management_begin_settings(revision, STAGE_QUOTA, Arc::new(ManagementJob::new()))?;
    match action {
        SettingsAction::CreateGroup { id, name } => {
            require_name(&name)?;
            if id.trim().is_empty() || id.len() > 128 {
                return Err("Недопустимый идентификатор группы.".into());
            }
            if name.len() > 1024 {
                return Err("Сократите название группы до 1024 байт.".into());
            }
            editor.create_group(&id, &name)?;
        }
        SettingsAction::DeleteGroup { handle: id } => editor.delete_group(handle(&id)?)?,
        SettingsAction::AppendMember { group, work } => {
            require_name(&work)?;
            editor.append_members(handle(&group)?, &[work])?;
        }
        SettingsAction::RemoveMember { group, handle: id } => {
            editor.remove_members(handle(&group)?, &[handle(&id)?])?;
        }
        SettingsAction::CreateCatalog { work, organization } => {
            require_name(&work)?;
            editor.create_catalog(&work, organization.as_deref())?;
        }
        SettingsAction::UpdateCatalog { handle: id, work, organization } => {
            require_name(&work)?;
            editor.update_catalog(handle(&id)?, &work, organization.as_deref())?;
        }
        SettingsAction::DeleteCatalog { handle: id } => editor.delete_catalog(&[handle(&id)?])?,
    }
    let receipt = store.management_commit_settings(editor)?;
    Ok(NativeSettingsReceipt {
        revision: receipt.revision,
        cleanup_warning: receipt.cleanup_error.is_some(),
    })
}

#[tauri::command]
pub async fn native_delete_report(
    app: AppHandle,
    revision: i64,
    id: String,
    workspace_token: Option<String>,
) -> Result<ManagementReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Write,
            |routed_storage| {
                let store = routed_storage;
                if id.len() > 65_536 || id.is_empty() {
                    return Err("Недопустимый идентификатор отчёта.".into());
                }
                store.management_delete_report(revision, &id, &ManagementJob::new())
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
}
