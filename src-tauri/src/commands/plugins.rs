//! Tauri command surface for the plugin system. Thin wrappers over
//! `PluginManager` — the frontend's `src/api/plugins.ts` mirrors this list.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;

use crate::db::DbPool;
use crate::executor::global_settings::load_global_settings;
use crate::plugins::{self, manifest::PluginManifest, PluginManager, PluginSummary};

#[tauri::command]
pub fn list_plugins(manager: State<'_, Arc<PluginManager>>) -> Vec<PluginSummary> {
    manager.list()
}

#[tauri::command]
pub fn get_plugin_manifest(
    plugin_id: String,
    manager: State<'_, Arc<PluginManager>>,
) -> Option<PluginManifest> {
    manager.manifest(&plugin_id)
}

#[tauri::command]
pub fn stage_plugin_install(path: String) -> Result<StagedInstall, String> {
    let (staging_id, manifest) = plugins::install::stage_from_zip(&PathBuf::from(path))?;
    Ok(StagedInstall {
        staging_id,
        manifest,
    })
}

#[tauri::command]
pub fn confirm_plugin_install(
    staging_id: String,
    app: tauri::AppHandle,
    manager: State<'_, Arc<PluginManager>>,
) -> Result<PluginManifest, String> {
    manager.confirm_install(&app, &staging_id)
}

#[tauri::command]
pub fn cancel_plugin_install(staging_id: String) -> Result<(), String> {
    plugins::install::cancel_staging(&staging_id)
}

#[tauri::command]
pub fn install_plugin_from_directory(
    path: String,
    app: tauri::AppHandle,
    manager: State<'_, Arc<PluginManager>>,
) -> Result<PluginManifest, String> {
    require_dev_mode()?;
    manager.install_from_directory(&app, &PathBuf::from(path))
}

#[tauri::command]
pub fn set_plugin_enabled(
    plugin_id: String,
    enabled: bool,
    app: tauri::AppHandle,
    manager: State<'_, Arc<PluginManager>>,
) -> Result<(), String> {
    manager.set_enabled(&app, &plugin_id, enabled)
}

#[tauri::command]
pub fn reload_plugin(
    plugin_id: String,
    app: tauri::AppHandle,
    manager: State<'_, Arc<PluginManager>>,
) -> Result<(), String> {
    manager.reload(&app, &plugin_id)
}

#[tauri::command]
pub fn reload_all_plugins(
    app: tauri::AppHandle,
    manager: State<'_, Arc<PluginManager>>,
) -> Result<(), String> {
    manager.reload_all(&app)
}

#[tauri::command]
pub fn uninstall_plugin(
    plugin_id: String,
    app: tauri::AppHandle,
    manager: State<'_, Arc<PluginManager>>,
) -> Result<(), String> {
    manager.uninstall(&app, &plugin_id)
}

#[tauri::command]
pub fn set_plugin_oauth_config(
    plugin_id: String,
    provider_id: String,
    client_id: String,
    client_secret: Option<String>,
) -> Result<(), String> {
    plugins::oauth::set_secret(
        &plugin_id,
        &format!("oauth.{}.client_id", provider_id),
        &client_id,
    )?;
    if let Some(secret) = client_secret {
        plugins::oauth::set_secret(
            &plugin_id,
            &format!("oauth.{}.client_secret", provider_id),
            &secret,
        )?;
    }
    Ok(())
}

#[tauri::command]
pub async fn start_plugin_oauth(
    plugin_id: String,
    provider_id: String,
    app: tauri::AppHandle,
    manager: State<'_, Arc<PluginManager>>,
) -> Result<(), String> {
    let manager = manager.inner().clone();
    plugins::oauth::start_flow(&app, &manager, &plugin_id, &provider_id).await
}

#[tauri::command]
pub fn disconnect_plugin_oauth(plugin_id: String, provider_id: String) -> Result<(), String> {
    plugins::oauth::disconnect(&plugin_id, &provider_id);
    Ok(())
}

#[tauri::command]
pub fn get_plugin_runtime_log(
    _plugin_id: String,
    _tail_lines: Option<u32>,
    _manager: State<'_, Arc<PluginManager>>,
) -> String {
    // Runtime log surfaces via `runtime::RuntimeRegistry` on the manager; the
    // registry is private because it's accessed exclusively from the runtime
    // module. We return empty until the runtime MCP client lands in a
    // follow-up slice and wires log delivery into the manager API.
    String::new()
}

#[tauri::command]
pub fn list_plugin_entities(
    plugin_id: String,
    entity_type: String,
    project_id: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
    db: State<'_, DbPool>,
) -> Result<Vec<plugins::entities::PluginEntity>, String> {
    let filter = plugins::entities::ListFilter {
        project_id,
        limit,
        offset,
    };
    plugins::entities::list(&db, &plugin_id, &entity_type, &filter)
}

#[tauri::command]
pub fn get_plugin_entity(
    id: String,
    db: State<'_, DbPool>,
) -> Result<Option<plugins::entities::PluginEntity>, String> {
    plugins::entities::get(&db, &id)
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedInstall {
    pub staging_id: String,
    pub manifest: PluginManifest,
}

fn require_dev_mode() -> Result<(), String> {
    let settings = load_global_settings();
    if !settings.developer.plugin_dev_mode {
        return Err("Dev-mode install requires developer.pluginDevMode = true".into());
    }
    Ok(())
}
