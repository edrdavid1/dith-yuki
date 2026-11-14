use super::*;
use tauri::State;

#[tauri::command]
pub fn channel_info(ch: State<'_, SharedChannel>) -> ChannelInfo {
    ch.info()
}

#[tauri::command]
pub fn entitlement(ch: State<'_, SharedChannel>) -> Entitlement {
    ch.entitlement()
}

#[tauri::command]
pub async fn activate_license(
    ch: State<'_, SharedChannel>,
    key: String,
) -> Result<Entitlement, String> {
    ch.activate(key).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn deactivate_license(ch: State<'_, SharedChannel>) -> Result<(), String> {
    ch.deactivate().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn refresh_license(ch: State<'_, SharedChannel>) -> Result<Entitlement, String> {
    ch.refresh().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_update(
    ch: State<'_, SharedChannel>,
    app: tauri::AppHandle,
) -> Result<Option<UpdateInfo>, String> {
    ch.check_update(app).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn install_update(
    ch: State<'_, SharedChannel>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    ch.install_update(app).await.map_err(|e| e.to_string())
}
