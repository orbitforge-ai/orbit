//! Tauri command surface for the plugin system. Thin wrappers over
//! `PluginManager` — the frontend's `src/api/plugins.ts` mirrors this list.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
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
pub async fn plugin_call_tool(
    plugin_id: String,
    tool_name: String,
    args: Option<Value>,
    _app: tauri::AppHandle,
    manager: State<'_, Arc<PluginManager>>,
) -> Result<Value, String> {
    let manifest = manager
        .manifest(&plugin_id)
        .ok_or_else(|| format!("plugin '{}' not installed", plugin_id))?;
    if !manager.is_enabled(&plugin_id) {
        return Err(format!("plugin '{}' is disabled", plugin_id));
    }
    if !manifest.tools.iter().any(|tool| tool.name == tool_name) {
        return Err(format!(
            "plugin '{}' does not expose a '{}' tool",
            plugin_id, tool_name
        ));
    }
    let extra_env = plugins::oauth::build_env_for_subprocess(&manifest);
    let raw = manager
        .runtime
        .call_tool(
            &manifest,
            &tool_name,
            &args.unwrap_or_else(|| Value::Object(Default::default())),
            &extra_env,
        )
        .await?;
    Ok(unwrap_mcp_text_payload(raw))
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
pub async fn reload_plugin(
    plugin_id: String,
    app: tauri::AppHandle,
    db: State<'_, DbPool>,
    manager: State<'_, Arc<PluginManager>>,
) -> Result<(), String> {
    manager.reload(&app, &plugin_id)?;
    // A reload replaces the subprocess; its in-memory subscription set is
    // empty until we re-apply it. Without this, a listening agent stops
    // matching inbound messages after any reload (secret save, dev edit, etc.).
    let db = db.inner().clone();
    crate::triggers::subscriptions::reconcile_plugin(&app, &db, &plugin_id).await;
    Ok(())
}

#[tauri::command]
pub async fn reload_all_plugins(
    app: tauri::AppHandle,
    db: State<'_, DbPool>,
    manager: State<'_, Arc<PluginManager>>,
) -> Result<(), String> {
    manager.reload_all(&app)?;
    let db = db.inner().clone();
    crate::triggers::subscriptions::reconcile_all(&app, &db).await;
    Ok(())
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
pub async fn set_plugin_secret(
    plugin_id: String,
    key: String,
    value: String,
    app: tauri::AppHandle,
    db: State<'_, DbPool>,
    manager: State<'_, Arc<PluginManager>>,
) -> Result<(), String> {
    let manifest = manager
        .manifest(&plugin_id)
        .ok_or_else(|| format!("plugin {:?} not installed", plugin_id))?;
    if !manifest.secrets.iter().any(|s| s.key == key) {
        return Err(format!(
            "plugin {:?} does not declare secret {:?}",
            plugin_id, key
        ));
    }
    plugins::oauth::set_secret(&plugin_id, &plugins::oauth::secret_account(&key), &value)?;
    // Reload so the subprocess picks up the new env var, then re-apply
    // subscriptions so a listening agent keeps matching inbound events.
    let _ = manager.reload(&app, &plugin_id);
    let db = db.inner().clone();
    crate::triggers::subscriptions::reconcile_plugin(&app, &db, &plugin_id).await;
    Ok(())
}

#[tauri::command]
pub async fn delete_plugin_secret(
    plugin_id: String,
    key: String,
    app: tauri::AppHandle,
    db: State<'_, DbPool>,
    manager: State<'_, Arc<PluginManager>>,
) -> Result<(), String> {
    plugins::oauth::delete_secret(&plugin_id, &plugins::oauth::secret_account(&key));
    let _ = manager.reload(&app, &plugin_id);
    let db = db.inner().clone();
    crate::triggers::subscriptions::reconcile_plugin(&app, &db, &plugin_id).await;
    Ok(())
}

#[tauri::command]
pub fn list_plugin_secret_status(
    manager: State<'_, Arc<PluginManager>>,
) -> Vec<PluginSecretStatus> {
    let mut out = Vec::new();
    for manifest in manager.manifests() {
        if manifest.secrets.is_empty() {
            continue;
        }
        let secrets: Vec<_> = manifest
            .secrets
            .iter()
            .map(|s| {
                let has_value = plugins::oauth::get_secret(
                    &manifest.id,
                    &plugins::oauth::secret_account(&s.key),
                )
                .is_ok();
                PluginSecretEntryStatus {
                    key: s.key.clone(),
                    display_name: s.display_name.clone(),
                    description: s.description.clone(),
                    placeholder: s.placeholder.clone(),
                    has_value,
                }
            })
            .collect();
        let any_needs_value = secrets.iter().any(|s| !s.has_value);
        out.push(PluginSecretStatus {
            plugin_id: manifest.id.clone(),
            any_needs_value,
            secrets,
        });
    }
    out
}

#[tauri::command]
pub fn get_plugin_runtime_log(
    plugin_id: String,
    tail_lines: Option<u32>,
    manager: State<'_, Arc<PluginManager>>,
) -> String {
    let n = tail_lines.unwrap_or(200) as usize;
    manager.runtime.log_tail(&plugin_id, n)
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

#[tauri::command]
pub fn list_plugin_oauth_status(manager: State<'_, Arc<PluginManager>>) -> Vec<PluginOAuthStatus> {
    let mut out = Vec::new();
    for manifest in manager.manifests() {
        if manifest.oauth_providers.is_empty() {
            continue;
        }
        let providers: Vec<_> = manifest
            .oauth_providers
            .iter()
            .map(|p| {
                let connected =
                    plugins::oauth::get_secret(&manifest.id, &format!("oauth.{}.access", p.id))
                        .is_ok();
                let has_client_id = p.client_id.is_some()
                    || plugins::oauth::get_secret(
                        &manifest.id,
                        &format!("oauth.{}.client_id", p.id),
                    )
                    .is_ok();
                PluginOAuthProviderStatus {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    client_type: p.client_type.clone(),
                    connected,
                    has_client_id,
                }
            })
            .collect();
        let any_needs_connect = providers.iter().any(|p| !p.connected);
        out.push(PluginOAuthStatus {
            plugin_id: manifest.id.clone(),
            any_needs_connect,
            providers,
        });
    }
    out
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginOAuthStatus {
    pub plugin_id: String,
    pub any_needs_connect: bool,
    pub providers: Vec<PluginOAuthProviderStatus>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginOAuthProviderStatus {
    pub id: String,
    pub name: String,
    pub client_type: String,
    pub connected: bool,
    pub has_client_id: bool,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedInstall {
    pub staging_id: String,
    pub manifest: PluginManifest,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSecretStatus {
    pub plugin_id: String,
    pub any_needs_value: bool,
    pub secrets: Vec<PluginSecretEntryStatus>,
}

fn unwrap_mcp_text_payload(raw: Value) -> Value {
    let text = raw
        .as_object()
        .and_then(|obj| obj.get("content"))
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("text"))
        .and_then(|t| t.as_str());
    if let Some(text) = text {
        if let Ok(parsed) = serde_json::from_str::<Value>(text) {
            return parsed;
        }
    }
    raw
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSecretEntryStatus {
    pub key: String,
    pub display_name: String,
    pub description: Option<String>,
    pub placeholder: Option<String>,
    pub has_value: bool,
}

fn require_dev_mode() -> Result<(), String> {
    let settings = load_global_settings();
    if !settings.developer.plugin_dev_mode {
        return Err("Dev-mode install requires developer.pluginDevMode = true".into());
    }
    Ok(())
}
