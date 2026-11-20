use crate::storage::{
    ManagementReport, ManagementReportFilter, QualityMatrixReportsPage, SqlPage, Storage,
};
use tauri::{AppHandle, Manager};

fn report_limit(limit: usize) -> Result<(), String> {
    if (1..=50).contains(&limit) {
        Ok(())
    } else {
        Err("Report page must contain 1..=50 rows".into())
    }
}

#[tauri::command]
pub async fn unified_report_window(
    app: AppHandle,
    revision: i64,
    filter: ManagementReportFilter,
    page_index: usize,
    limit: usize,
) -> Result<SqlPage<ManagementReport>, String> {
    report_limit(limit)?;
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>()
            .management_report_window(revision, &filter, page_index, limit)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_report_org_window(
    app: AppHandle,
    revision: i64,
    id: String,
    page_index: usize,
    limit: usize,
) -> Result<SqlPage<(i64, String)>, String> {
    report_limit(limit)?;
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>()
            .management_report_org_window(revision, &id, page_index, limit)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn native_quality_matrix_reports_at_page(
    app: AppHandle,
    revision: i64,
    report_type: String,
    start: String,
    organization: Option<String>,
    page_index: usize,
    limit: usize,
) -> Result<QualityMatrixReportsPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().quality_matrix_reports_at_page(
            revision,
            &report_type,
            &start,
            organization.as_deref(),
            page_index,
            limit,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
