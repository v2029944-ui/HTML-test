use crate::{native_advanced_analysis::AdvancedRequest, storage::{RangeQuery, Storage, TrendCsvOptions}};
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn reference_trend_csv(
    app: AppHandle, request: AdvancedRequest, options: TrendCsvOptions, destination: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Storage>();
        let range = RangeQuery { from: request.query.from.clone(), to: request.query.to.clone() };
        request.with_scope(&store, |scope, grouping| {
            store.reference_trend_csv(request.revision, scope, &range, grouping, &options, std::path::Path::new(&destination))
        })
    }).await.map_err(|e| e.to_string())?
}
