#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod storage;

use storage::{ReadExcel, Rule, Storage};
use tauri::{Emitter, Manager, State, Url};
use tauri_plugin_updater::UpdaterExt;

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


const BACKUP_UPDATE_URL: &str =
    "https://storage.yandexcloud.net/html-test-updates-23727/tauri-latest.json";
const BACKUP_PROGRESS_EVENT: &str = "html-test-backup-update-progress";

fn backup_updater(app: &tauri::AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    // A separate, trusted native updater keeps the existing signing key and
    // bypasses primary-endpoint connection failures, including timeouts.
    let url: Url = BACKUP_UPDATE_URL.parse().map_err(|e| format!("Резервный адрес: {e}"))?;
    app.updater_builder()
        .endpoints(vec![url]).map_err(|e| e.to_string())?
        .timeout(std::time::Duration::from_secs(15))
        .build().map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupUpdate {
    version: String,
    current_version: String,
    body: Option<String>,
}

#[tauri::command]
async fn check_backup_update(app: tauri::AppHandle) -> Result<Option<BackupUpdate>, String> {
    if !updater_configured() {
        return Err("Проверка подписи обновления не настроена.".into());
    }
    Ok(backup_updater(&app)?
        .check().await.map_err(|e| e.to_string())?
        .map(|found| BackupUpdate {
            version: found.version,
            current_version: found.current_version,
            body: found.body,
        }))
}

#[tauri::command]
async fn install_backup_update(
    app: tauri::AppHandle,
    expected_version: String,
) -> Result<(), String> {
    if !updater_configured() {
        return Err("Проверка подписи обновления не настроена.".into());
    }
    // Re-check the trusted Yandex manifest immediately before installing.
    // Never install a different version than the one the user approved.
    let found = backup_updater(&app)?
        .check().await.map_err(|e| e.to_string())?
        .ok_or("Резервный источник больше не предлагает обновление.")?;
    if found.version != expected_version {
        return Err("Версия резервного обновления изменилась. Проверьте обновление заново.".into());
    }
    let chunk_app = app.clone();
    let finished_app = app.clone();
    found.download_and_install(
        move |chunk_length, content_length| {
            let _ = chunk_app.emit(BACKUP_PROGRESS_EVENT, serde_json::json!({
                "event": "Progress",
                "data": {"chunkLength": chunk_length, "contentLength": content_length}
            }));
        },
        move || {
            let _ = finished_app.emit(BACKUP_PROGRESS_EVENT, serde_json::json!({
                "event": "Finished"
            }));
        },
    ).await.map_err(|e| e.to_string())
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
            updater_configured, check_backup_update, install_backup_update,
            load_data, save_data, get_info, set_folder, set_rule,
            list_excel, read_excel, check_excel, open_data_folder
        ])
        .run(tauri::generate_context!())
        .expect("Failed to start Tauri application");
}

