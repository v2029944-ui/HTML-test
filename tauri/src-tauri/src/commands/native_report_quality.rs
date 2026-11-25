use crate::storage::{QualityScope, ReportQualityPage};
use tauri::AppHandle;

#[tauri::command]
pub async fn native_report_quality_page(
    app: AppHandle,
    revision: i64,
    id: String,
    scope: QualityScope,
    offset: u32,
    limit: usize,
    workspace_token: Option<String>,
) -> Result<ReportQualityPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                routed_storage.report_quality_page(revision, &id, scope, offset, limit)
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
}
