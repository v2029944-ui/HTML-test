#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod storage;

use storage::{ReadExcel, Rule, Storage};
use tauri::{Manager, State};

#[tauri::command]
fn load_data(store: State<'_, Storage>) -> Result<serde_json::Value, String> {
    store.load()
}

#[tauri::command]
fn save_data(store: State<'_, Storage>, data: serde_json::Value) -> Result<serde_json::Value, String> {
    store.save(data)
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
            updater_configured, load_data, save_data, get_info, set_folder, set_rule,
            list_excel, read_excel, check_excel, open_data_folder
        ])
        .run(tauri::generate_context!())
        .expect("Failed to start Tauri application");
}

