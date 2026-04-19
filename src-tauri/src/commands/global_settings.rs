use crate::executor::global_settings::{self, GlobalSettings};

#[tauri::command]
pub async fn get_global_settings() -> Result<GlobalSettings, String> {
    tokio::task::spawn_blocking(global_settings::load_global_settings)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_global_settings(settings: GlobalSettings) -> Result<GlobalSettings, String> {
    tokio::task::spawn_blocking(move || global_settings::save_global_settings(settings))
        .await
        .map_err(|e| e.to_string())?
}
