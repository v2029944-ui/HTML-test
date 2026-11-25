use crate::storage::{QualityMatrixPage, QualityMatrixReportsPage};

#[tauri::command]
pub async fn native_quality_matrix_page(
    app: tauri::AppHandle,
    revision: i64,
    report_type: String,
    from: String,
    to: String,
    organizations: Vec<String>,
    workspace_token: Option<String>,
) -> Result<QualityMatrixPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                routed_storage.quality_matrix_page(
                    revision,
                    &report_type,
                    &from,
                    &to,
                    &organizations,
                )
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn native_quality_matrix_reports(
    app: tauri::AppHandle,
    revision: i64,
    report_type: String,
    start: String,
    organization: Option<String>,
    after: Option<String>,
    limit: usize,
    workspace_token: Option<String>,
) -> Result<QualityMatrixReportsPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                routed_storage.quality_matrix_reports(
                    revision,
                    &report_type,
                    &start,
                    organization.as_deref(),
                    after.as_deref(),
                    limit,
                )
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
}
