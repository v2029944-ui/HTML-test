use crate::{native_advanced_analysis::AdvancedRequest, storage::TrendCsvOptions};
use tauri::AppHandle;

#[tauri::command]
pub async fn reference_trend_csv(
    app: AppHandle,
    request: AdvancedRequest,
    options: TrendCsvOptions,
    destination: String,
    workspace_token: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                let store = routed_storage;
                request.with_range_scope(&store, |revision, scope, range, grouping| {
                    store.reference_trend_csv(
                        revision,
                        scope,
                        range,
                        grouping,
                        &options,
                        std::path::Path::new(&destination),
                    )
                })
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
