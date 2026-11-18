#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod storage;

use storage::{ManagementJob, ReadExcel, ReadFilter, ReportHeader, Rule, Storage};
use std::sync::Arc;
use tauri::{Manager, State};

#[tauri::command]
fn load_data(store: State<'_, Storage>) -> Result<serde_json::Value, String> {
    store.load()
}

#[tauri::command]
fn save_data(store: State<'_, Storage>, data: serde_json::Value) -> Result<serde_json::Value, String> {
    store.save(data)
}


#[derive(serde::Deserialize)]
struct NativeReportCursor {
    start: String,
    id: String,
}

// SQLite reads are executed away from the WebView thread. The existing D1
// backend guarantees bounded metadata pages and revision-checked keyset order.
#[tauri::command]
async fn native_read_revision(app: tauri::AppHandle) -> Result<i64, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Storage>().revision())
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn native_report_page(
    app: tauri::AppHandle,
    revision: i64,
    filter: ReadFilter,
    after: Option<NativeReportCursor>,
    limit: usize,
) -> Result<Vec<ReportHeader>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().report_page(
            revision,
            &filter,
            after.as_ref().map(|cursor| (cursor.start.as_str(), cursor.id.as_str())),
            limit,
        )
    })
    .await
    .map_err(|error| error.to_string())?
}


#[derive(serde::Serialize)]
struct NativeGroup {
    handle: String,
    id: String,
    name: String,
    members: i64,
}

#[tauri::command]
async fn native_settings_bootstrap(
    app: tauri::AppHandle,
) -> Result<storage::ManagementBootstrap, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Storage>();
        let revision = store.revision()?;
        let job = ManagementJob::new();
        // A bounded, streaming settings index, never the full reports JSON.
        store.management_index_settings(revision, &job)?;
        store.management_bootstrap(revision)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn native_group_page(
    app: tauri::AppHandle,
    revision: i64,
    after_handle: Option<String>,
    limit: usize,
) -> Result<Vec<NativeGroup>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let after = after_handle
            .as_deref()
            .map(|value| value.parse::<i64>().map_err(|_| "Invalid group cursor".to_string()))
            .transpose()?;
        let groups = app.state::<Storage>().management_group_page(revision, "", after, limit)?;
        Ok(groups.into_iter().map(|group| NativeGroup {
            handle: group.handle.to_string(),
            id: group.id,
            name: group.name,
            members: group.members,
        }).collect())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn native_group_rename(
    app: tauri::AppHandle,
    revision: i64,
    handle: String,
    name: String,
) -> Result<i64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if name.trim().is_empty() || name.len() > 1024 {
            return Err("Group name must contain 1..1024 UTF-8 bytes".to_string());
        }
        let handle = handle.parse::<i64>()
            .map_err(|_| "Invalid group handle".to_string())?;
        let store = app.state::<Storage>();
        let job = Arc::new(ManagementJob::new());
        let mut editor = store.management_begin_settings(
            revision, 256 * 1024 * 1024, job,
        )?;
        editor.rename_group(handle, &name)?;
        // Settings-only transactional publication. No report write or full load.
        Ok(store.management_commit_settings(editor)?.revision)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn get_info(store: State<'_, Storage>) -> Result<serde_json::Value, String> {
    store.info()
}

#[tauri::command]
fn set_folder(store: State<'_, Storage>, folder: String) -> Result<serde_json::Value, String> {
    store.set_folder(folder)
}

#[tauri::command]
fn set_rule(store: State<'_, Storage>, name: String, rule: Rule) -> Result<Rule, String> {
    store.set_rule(name, rule)
}

#[tauri::command]
fn list_excel(store: State<'_, Storage>) -> Result<serde_json::Value, String> {
    store.list_excel()
}

#[tauri::command]
fn read_excel(store: State<'_, Storage>, name: String) -> Result<ReadExcel, String> {
    store.read_excel(name)
}

#[tauri::command]
fn check_excel(store: State<'_, Storage>, name: String, signature: String) -> bool {
    store.check_excel(name, signature)
}

#[tauri::command]
fn open_data_folder(store: State<'_, Storage>) -> Result<String, String> {
    tauri_plugin_opener::open_path(store.root(), None::<&str>).map_err(|e| e.to_string())?;
    Ok(String::new())
}

#[tauri::command]
fn updater_configured() -> bool {
    !option_env!("ANALIZ_UPDATER_PUBLIC_KEY").unwrap_or("").trim().is_empty()
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new()
            .pubkey(option_env!("ANALIZ_UPDATER_PUBLIC_KEY").unwrap_or(""))
            .build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let root = app.path().document_dir()?.join("Анализ работ");
            let storage = Storage::new(root)
                .map_err(std::io::Error::other)?;
            app.manage(storage);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            updater_configured, load_data, save_data, native_read_revision, native_report_page,
            native_settings_bootstrap, native_group_page, native_group_rename,
            get_info, set_folder, set_rule,
            list_excel, read_excel, check_excel, open_data_folder
        ])
        .run(tauri::generate_context!())
        .expect("Failed to start Tauri application");
}

