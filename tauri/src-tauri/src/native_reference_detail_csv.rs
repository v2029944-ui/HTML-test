//! Export the same filtered table that the installed UI is displaying.
use crate::{
    native_advanced_analysis::AdvancedRequest,
    storage::{DetailCsvOptions},
};
use serde::Deserialize;
use tauri::AppHandle;

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
    workspace_token: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                let store = routed_storage;
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
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
