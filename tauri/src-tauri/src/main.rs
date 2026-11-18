#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod storage;

use storage::{ReadExcel, ReadFilter, ReportHeader, Rule, Storage};
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
            get_info, set_folder, set_rule,
            list_excel, read_excel, check_excel, open_data_folder
        ])
        .run(tauri::generate_context!())
        .expect("Failed to start Tauri application");
}

