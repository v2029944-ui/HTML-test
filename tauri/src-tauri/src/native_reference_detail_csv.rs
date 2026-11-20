//! Export the same filtered table that the installed UI is displaying.
use crate::{
    native_advanced_analysis::AdvancedRequest,
    storage::{DetailCsvOptions, Storage},
};
use serde::Deserialize;
use tauri::{AppHandle, Manager};

#[derive(Deserialize)]
pub struct DetailCsvRequest {
    request: AdvancedRequest,
    #[serde(flatten)]
    table: DetailCsvOptions,
}

#[tauri::command]
pub async fn reference_detail_csv(
    app: AppHandle,
    payload: DetailCsvRequest,
    destination: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Storage>();
        let request = &payload.request;
        request.with_scope(&store, |scope, grouping| {
            store.reference_detail_csv(
                request.revision,
                scope,
                &request.query,
                grouping,
                &payload.table,
                std::path::Path::new(&destination),
            )
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
