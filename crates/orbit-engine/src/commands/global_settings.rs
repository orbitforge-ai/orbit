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

mod http {
    use super::*;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UpdateArgs {
        settings: GlobalSettings,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("get_global_settings", |_ctx, _args| async move {
            let r = get_global_settings().await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("update_global_settings", |_ctx, args| async move {
            let a: UpdateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = update_global_settings(a.settings).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
    }
}

pub use http::register as register_http;
