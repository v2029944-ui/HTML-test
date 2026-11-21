use crate::storage::{
    shared_runtime::{Access, Runtime, WorkspaceStatus},
    Storage,
};
use tauri::{Manager, Runtime as TauriRuntime};
use tauri_plugin_dialog::DialogExt;

pub fn execute<T, R: TauriRuntime, M: Manager<R>>(
    manager: &M,
    token: Option<&str>,
    access: Access,
    action: impl FnOnce(&Storage) -> Result<T, String>,
) -> Result<T, String> {
    manager.state::<Runtime>().execute(token, access, action)
}
#[tauri::command]
pub async fn shared_workspace_status(app: tauri::AppHandle) -> Result<WorkspaceStatus, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Runtime>().status())
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn shared_workspace_choose_folder(
    app: tauri::AppHandle,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let parent = app
            .get_webview_window("main")
            .ok_or("Главное окно недоступно.")?;
        let Some(folder) = app
            .dialog()
            .file()
            .set_title("Общая папка команды")
            .set_parent(&parent)
            .blocking_pick_folder()
        else {
            return Ok(None);
        };
        let folder = folder.into_path().map_err(|e| e.to_string())?;
        app.state::<Runtime>().grant(folder.clone())?;
        Ok(Some(folder.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn shared_workspace_connect(
    app: tauri::AppHandle,
    folder: String,
) -> Result<WorkspaceStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Runtime>()
            .connect(std::path::Path::new(&folder))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn shared_workspace_sync(app: tauri::AppHandle) -> Result<WorkspaceStatus, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Runtime>().sync())
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn shared_workspace_disconnect(app: tauri::AppHandle) -> Result<WorkspaceStatus, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Runtime>().disconnect())
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn shared_workspace_discard_pending(
    app: tauri::AppHandle,
    workspace_token: Option<String>,
) -> Result<WorkspaceStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Runtime>()
            .discard_pending(workspace_token.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}
