//! Tauri command surface for the plugin system. Thin wrappers over
//! `PluginManager` — the frontend's `src/api/plugins.ts` mirrors this list.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use futures::future::join_all;
use serde::Deserialize;
use serde_json::Value;
use tauri::State;
use tokio::time::timeout;

use crate::db::DbPool;
use crate::executor::global_settings::load_global_settings;
use crate::plugins::{
    self,
    manifest::{PluginManifest, SurfaceActionSpec, SurfaceActionSurface},
    PluginManager, PluginSummary,
};

const SURFACE_ACTION_RESOLVER_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SurfaceActionCacheKey {
    plugin_id: String,
    contribution_id: String,
    surface: SurfaceActionSurface,
    path: Option<String>,
}

#[derive(Debug, Clone)]
struct CachedSurfaceActions {
    actions: Vec<PluginSurfaceAction>,
}

static SURFACE_ACTION_CACHE: OnceLock<
    RwLock<HashMap<SurfaceActionCacheKey, CachedSurfaceActions>>,
> = OnceLock::new();

fn surface_action_cache() -> &'static RwLock<HashMap<SurfaceActionCacheKey, CachedSurfaceActions>> {
    SURFACE_ACTION_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceActionTarget {
    pub kind: String,
    pub token: String,
    pub display_path: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceActionPromptField {
    pub name: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSurfaceActionItem {
    pub id: String,
    pub label: String,
    pub disabled: bool,
    pub target: SurfaceActionTarget,
    pub tool: String,
    pub args: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<Vec<SurfaceActionPromptField>>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSurfaceAction {
    pub id: String,
    pub plugin_id: String,
    pub plugin_name: String,
    pub contribution_id: String,
    pub presentation: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub hide_label: bool,
    pub tooltip: Option<String>,
    pub disabled: bool,
    pub stale: bool,
    pub target: Option<SurfaceActionTarget>,
    pub tool: Option<String>,
    pub args: Option<Value>,
    pub items: Vec<PluginSurfaceActionItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<Vec<SurfaceActionPromptField>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolverSurfaceActionPayload {
    #[serde(default)]
    actions: Vec<ResolverSurfaceAction>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolverSurfaceAction {
    id: String,
    presentation: String,
    label: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    hide_label: bool,
    #[serde(default)]
    tooltip: Option<String>,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    target: Option<SurfaceActionTarget>,
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    args: Option<Value>,
    #[serde(default)]
    items: Vec<ResolverSurfaceActionItem>,
    #[serde(default)]
    prompt: Option<Vec<SurfaceActionPromptField>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolverSurfaceActionItem {
    id: String,
    label: String,
    #[serde(default)]
    disabled: bool,
    target: SurfaceActionTarget,
    tool: String,
    #[serde(default)]
    args: Option<Value>,
    #[serde(default)]
    prompt: Option<Vec<SurfaceActionPromptField>>,
}

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
pub async fn list_plugin_surface_actions(
    surface: SurfaceActionSurface,
    path: Option<String>,
    manager: State<'_, Arc<PluginManager>>,
) -> Result<Vec<PluginSurfaceAction>, String> {
    let manager = manager.inner().clone();
    let manifests: Vec<_> = manager
        .list()
        .into_iter()
        .filter(|summary| summary.enabled)
        .filter_map(|summary| {
            manager
                .manifest(&summary.id)
                .map(|manifest| (summary.name, manifest))
        })
        .collect();

    let mut tasks = Vec::new();
    for (plugin_name, manifest) in manifests {
        let tool_names: Vec<String> = manifest
            .tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect();
        let extra_env = plugins::oauth::build_env_for_subprocess(&manifest);
        let matching: Vec<SurfaceActionSpec> = manifest
            .ui
            .surface_actions
            .iter()
            .filter(|spec| spec.surface == surface)
            .cloned()
            .collect();
        for spec in matching {
            let manager = manager.clone();
            let manifest = manifest.clone();
            let plugin_name = plugin_name.clone();
            let extra_env = extra_env.clone();
            let tool_names = tool_names.clone();
            let path = path.clone();
            tasks.push(async move {
                resolve_surface_action_spec(
                    manager,
                    manifest,
                    plugin_name,
                    spec,
                    path,
                    tool_names,
                    extra_env,
                )
                .await
            });
        }
    }

    let results = join_all(tasks).await;
    Ok(results.into_iter().flatten().collect())
}

#[tauri::command]
pub async fn run_plugin_surface_action(
    plugin_id: String,
    tool_name: String,
    args: Option<Value>,
    surface: SurfaceActionSurface,
    target: SurfaceActionTarget,
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
    validate_surface_action_target(&target)?;
    let args = match args {
        Some(Value::Object(map)) => map,
        Some(_) => return Err("surface action args must be a JSON object".into()),
        None => serde_json::Map::new(),
    };
    let mut input = args;
    input.insert(
        "context".into(),
        serde_json::json!({
            "surface": surface,
            "target": target,
            "invocationMode": "surfaceAction",
        }),
    );

    let extra_env = plugins::oauth::build_env_for_subprocess(&manifest);
    let raw = manager
        .runtime
        .call_tool(&manifest, &tool_name, &Value::Object(input), &extra_env)
        .await?;
    Ok(unwrap_mcp_text_payload(raw))
}

#[tauri::command]
pub fn stage_plugin_install(path: String) -> Result<StagedInstall, String> {
    let (staging_id, mut manifest) = plugins::install::stage_from_zip(&PathBuf::from(path))?;
    plugins::manifest::populate_icon_data_url(
        &mut manifest,
        &plugins::staging_dir().join(&staging_id),
    );
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
    let mut manifest = manager.confirm_install(&app, &staging_id)?;
    let plugin_id = manifest.id.clone();
    plugins::manifest::populate_icon_data_url(
        &mut manifest,
        &plugins::install::resolve_source_dir(&plugin_id),
    );
    Ok(manifest)
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
    let source = PathBuf::from(path);
    let mut manifest = manager.install_from_directory(&app, &source)?;
    plugins::manifest::populate_icon_data_url(&mut manifest, &source);
    Ok(manifest)
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

async fn resolve_surface_action_spec(
    manager: Arc<PluginManager>,
    manifest: PluginManifest,
    plugin_name: String,
    spec: SurfaceActionSpec,
    path: Option<String>,
    tool_names: Vec<String>,
    extra_env: std::collections::BTreeMap<String, String>,
) -> Vec<PluginSurfaceAction> {
    let cache_key = SurfaceActionCacheKey {
        plugin_id: manifest.id.clone(),
        contribution_id: spec.id.clone(),
        surface: spec.surface,
        path: path.clone(),
    };
    let request = serde_json::json!({
        "surface": spec.surface,
        "path": path,
    });

    let call = timeout(
        SURFACE_ACTION_RESOLVER_TIMEOUT,
        manager
            .runtime
            .call_tool(&manifest, &spec.resolve_tool, &request, &extra_env),
    )
    .await;

    match call {
        Ok(Ok(raw)) => {
            let parsed = unwrap_mcp_text_payload(raw);
            match normalize_surface_actions(&manifest.id, &plugin_name, &spec, parsed, &tool_names)
            {
                Ok(actions) => {
                    if let Ok(mut cache) = surface_action_cache().write() {
                        cache.insert(
                            cache_key,
                            CachedSurfaceActions {
                                actions: actions.clone(),
                            },
                        );
                    }
                    tracing::debug!(
                        plugin_id = manifest.id.as_str(),
                        contribution_id = spec.id.as_str(),
                        fresh = true,
                        "resolved plugin surface actions"
                    );
                    actions
                }
                Err(err) => {
                    tracing::warn!(
                        plugin_id = manifest.id.as_str(),
                        contribution_id = spec.id.as_str(),
                        "invalid surface action payload: {}",
                        err
                    );
                    cached_surface_actions(&cache_key, true)
                }
            }
        }
        Ok(Err(err)) => {
            tracing::warn!(
                plugin_id = manifest.id.as_str(),
                contribution_id = spec.id.as_str(),
                "surface action resolver failed: {}",
                err
            );
            cached_surface_actions(&cache_key, true)
        }
        Err(_) => {
            tracing::warn!(
                plugin_id = manifest.id.as_str(),
                contribution_id = spec.id.as_str(),
                "surface action resolver timed out after {}ms",
                SURFACE_ACTION_RESOLVER_TIMEOUT.as_millis()
            );
            cached_surface_actions(&cache_key, true)
        }
    }
}

fn cached_surface_actions(
    cache_key: &SurfaceActionCacheKey,
    stale: bool,
) -> Vec<PluginSurfaceAction> {
    let Ok(cache) = surface_action_cache().read() else {
        return Vec::new();
    };
    let Some(entry) = cache.get(cache_key) else {
        return Vec::new();
    };
    let mut actions = entry.actions.clone();
    for action in &mut actions {
        action.stale = stale;
    }
    actions
}

fn normalize_surface_actions(
    plugin_id: &str,
    plugin_name: &str,
    spec: &SurfaceActionSpec,
    payload: Value,
    tool_names: &[String],
) -> Result<Vec<PluginSurfaceAction>, String> {
    let parsed: ResolverSurfaceActionPayload = serde_json::from_value(payload)
        .map_err(|e| format!("resolver payload is invalid: {}", e))?;
    let declared_tools: std::collections::HashSet<&str> =
        tool_names.iter().map(|name| name.as_str()).collect();
    let mut out = Vec::new();

    for action in parsed.actions {
        if action.id.trim().is_empty() {
            tracing::warn!(
                plugin_id,
                contribution_id = spec.id.as_str(),
                "dropping surface action with empty id"
            );
            continue;
        }
        if action.label.trim().is_empty() {
            tracing::warn!(
                plugin_id,
                contribution_id = spec.id.as_str(),
                action_id = action.id.as_str(),
                "dropping surface action with empty label"
            );
            continue;
        }

        match action.presentation.as_str() {
            "button" => {
                let Some(tool) = action.tool else {
                    tracing::warn!(
                        plugin_id,
                        contribution_id = spec.id.as_str(),
                        action_id = action.id.as_str(),
                        "dropping button surface action without tool"
                    );
                    continue;
                };
                if !declared_tools.contains(tool.as_str()) {
                    tracing::warn!(
                        plugin_id,
                        contribution_id = spec.id.as_str(),
                        action_id = action.id.as_str(),
                        tool_name = tool.as_str(),
                        "dropping button surface action with undeclared tool"
                    );
                    continue;
                }
                let Some(target) = action.target else {
                    tracing::warn!(
                        plugin_id,
                        contribution_id = spec.id.as_str(),
                        action_id = action.id.as_str(),
                        "dropping button surface action without target"
                    );
                    continue;
                };
                if validate_surface_action_target(&target).is_err() {
                    tracing::warn!(
                        plugin_id,
                        contribution_id = spec.id.as_str(),
                        action_id = action.id.as_str(),
                        "dropping button surface action with invalid target"
                    );
                    continue;
                }
                let args = normalize_surface_action_args(action.args).map_err(|err| {
                    format!(
                        "button action {:?} returned invalid args: {}",
                        action.id, err
                    )
                })?;
                out.push(PluginSurfaceAction {
                    id: format!("{}:{}:{}", plugin_id, spec.id, action.id),
                    plugin_id: plugin_id.to_string(),
                    plugin_name: plugin_name.to_string(),
                    contribution_id: spec.id.clone(),
                    presentation: "button".into(),
                    label: action.label,
                    icon: action.icon,
                    hide_label: action.hide_label,
                    tooltip: action.tooltip,
                    disabled: action.disabled,
                    stale: false,
                    target: Some(target),
                    tool: Some(tool),
                    args: Some(args),
                    items: Vec::new(),
                    prompt: action.prompt,
                });
            }
            "menu" => {
                let mut items = Vec::new();
                for item in action.items {
                    if item.id.trim().is_empty() || item.label.trim().is_empty() {
                        continue;
                    }
                    if !declared_tools.contains(item.tool.as_str()) {
                        tracing::warn!(
                            plugin_id,
                            contribution_id = spec.id.as_str(),
                            action_id = action.id.as_str(),
                            item_id = item.id.as_str(),
                            tool_name = item.tool.as_str(),
                            "dropping menu item with undeclared tool"
                        );
                        continue;
                    }
                    if validate_surface_action_target(&item.target).is_err() {
                        tracing::warn!(
                            plugin_id,
                            contribution_id = spec.id.as_str(),
                            action_id = action.id.as_str(),
                            item_id = item.id.as_str(),
                            "dropping menu item with invalid target"
                        );
                        continue;
                    }
                    let args = normalize_surface_action_args(item.args).map_err(|err| {
                        format!(
                            "menu action {:?} item {:?} returned invalid args: {}",
                            action.id, item.id, err
                        )
                    })?;
                    items.push(PluginSurfaceActionItem {
                        id: format!("{}:{}:{}:{}", plugin_id, spec.id, action.id, item.id),
                        label: item.label,
                        disabled: item.disabled,
                        target: item.target,
                        tool: item.tool,
                        args,
                        prompt: item.prompt,
                    });
                }
                if items.is_empty() {
                    continue;
                }
                out.push(PluginSurfaceAction {
                    id: format!("{}:{}:{}", plugin_id, spec.id, action.id),
                    plugin_id: plugin_id.to_string(),
                    plugin_name: plugin_name.to_string(),
                    contribution_id: spec.id.clone(),
                    presentation: "menu".into(),
                    label: action.label,
                    icon: action.icon,
                    hide_label: action.hide_label,
                    tooltip: action.tooltip,
                    disabled: action.disabled,
                    stale: false,
                    target: None,
                    tool: None,
                    args: None,
                    items,
                    prompt: None,
                });
            }
            other => {
                tracing::warn!(
                    plugin_id,
                    contribution_id = spec.id.as_str(),
                    action_id = action.id.as_str(),
                    presentation = other,
                    "dropping surface action with unsupported presentation"
                );
            }
        }
    }

    Ok(out)
}

fn normalize_surface_action_args(args: Option<Value>) -> Result<Value, String> {
    match args {
        Some(Value::Object(map)) => Ok(Value::Object(map)),
        Some(Value::Null) | None => Ok(Value::Object(Default::default())),
        Some(_) => Err("args must be a JSON object".into()),
    }
}

fn validate_surface_action_target(target: &SurfaceActionTarget) -> Result<(), String> {
    if target.kind.trim().is_empty() {
        return Err("target.kind must not be empty".into());
    }
    if target.token.trim().is_empty() {
        return Err("target.token must not be empty".into());
    }
    Ok(())
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
