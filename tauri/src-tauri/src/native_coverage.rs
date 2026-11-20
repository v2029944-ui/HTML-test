use crate::storage::{QualityMatrixPage, QualityMatrixReportsPage, Storage};
use tauri::Manager;

#[tauri::command]
pub async fn native_quality_matrix_page(
    app: tauri::AppHandle,
    revision: i64,
    report_type: String,
    from: String,
    to: String,
    organizations: Vec<String>,
) -> Result<QualityMatrixPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().quality_matrix_page(
            revision,
            &report_type,
            &from,
            &to,
            &organizations,
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
) -> Result<QualityMatrixReportsPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().quality_matrix_reports(
            revision,
            &report_type,
            &start,
            organization.as_deref(),
            after.as_deref(),
            limit,
        )
    })
    .await
    .map_err(|error| error.to_string())?
}
