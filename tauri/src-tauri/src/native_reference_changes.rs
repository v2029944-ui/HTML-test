//! IPC adapters for explicit independent reference comparison intervals.
use crate::storage::{ReferenceChangeRow, ReferenceChangesPage, ReferenceChangesRequest};
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc};
use std::time::Duration;
use tauri::{AppHandle, Listener};

#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PrintEvent {
    Ready { control: String },
    Rows { offset: usize, rows: Vec<ReferenceChangeRow> },
    Done,
}

#[tauri::command]
pub async fn reference_changes(
    app: AppHandle,
    request: ReferenceChangesRequest,
    workspace_token: Option<String>,
) -> Result<ReferenceChangesPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| routed_storage.reference_changes(&request),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}
#[tauri::command]
pub async fn reference_changes_print(
    app: AppHandle,
    request: ReferenceChangesRequest,
    batches: tauri::ipc::Channel<PrintEvent>,
    workspace_token: Option<String>,
) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // Renderer acknowledgements and cancellation bypass the workspace data mutex.
        let control = format!("reference-print-{}", batches.id());
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancelled);
        let (sender, replies) = mpsc::sync_channel(1);
        let listener = app.listen(control.clone(), move |event| {
            if let Ok(reply) = serde_json::from_str::<Option<usize>>(event.payload()) {
                if reply.is_none() {
                    stop.store(true, Ordering::Release);
                }
                let _ = sender.try_send(reply);
            }
        });
        let run = || {
            batches.send(PrintEvent::Ready { control }).map_err(|e| e.to_string())?;
            let comparable = crate::native_shared_runtime::execute(
                &app,
                workspace_token.as_deref(),
                crate::storage::shared_runtime::Access::Read,
                |storage| {
                    let mut offset = 0;
                    storage.reference_changes_print(&request, Arc::clone(&cancelled), |rows| {
                        let end = offset + rows.len();
                        batches.send(PrintEvent::Rows { offset, rows }).map_err(|e| e.to_string())?;
                        let reply = replies.recv_timeout(Duration::from_secs(30))
                            .map_err(|_| "Прервано получение строк печати.".to_string())?;
                        if reply != Some(end) || cancelled.load(Ordering::Acquire) {
                            return Err("Печать отменена.".into());
                        }
                        offset = end;
                        Ok(())
                    })
                },
            )?;
            batches.send(PrintEvent::Done).map_err(|e| e.to_string())?;
            Ok(comparable)
        };
        let result = run();
        app.unlisten(listener);
        result
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn reference_changes_csv(
    app: AppHandle,
    request: ReferenceChangesRequest,
    destination: String,
    workspace_token: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                routed_storage.reference_changes_csv(&request, std::path::Path::new(&destination))
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn reference_summary_csv(
    app: AppHandle,
    request: ReferenceChangesRequest,
    destination: String,
    workspace_token: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                routed_storage.reference_summary_csv(&request, std::path::Path::new(&destination))
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
}
