use crate::storage::{QualityScope, ReportQualityPage, Storage};
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn native_report_quality_page(
    app: AppHandle,
    revision: i64,
    id: String,
    scope: QualityScope,
    offset: u32,
    limit: usize,
) -> Result<ReportQualityPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().report_quality_page(revision, &id, scope, offset, limit)
    })
    .await
    .map_err(|error| error.to_string())?
}
