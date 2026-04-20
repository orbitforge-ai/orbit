//! Tauri commands for binding channels to agents and exposing plugin
//! channel-picker data.

use serde_json::{json, Value};

use crate::db::DbPool;
use crate::executor::workspace;
use crate::models::channel_binding::ChannelBinding;
use crate::plugins;
use crate::triggers::subscriptions;

#[tauri::command]
pub async fn list_agent_listen_bindings(agent_id: String) -> Result<Vec<ChannelBinding>, String> {
    let cfg = workspace::load_agent_config(&agent_id).map_err(|e| e.to_string())?;
    Ok(cfg.listen_bindings)
}

#[tauri::command]
pub async fn set_agent_listen_bindings(
    app: tauri::AppHandle,
    agent_id: String,
    bindings: Vec<ChannelBinding>,
    db: tauri::State<'_, DbPool>,
) -> Result<(), String> {
    let mut cfg = workspace::load_agent_config(&agent_id).map_err(|e| e.to_string())?;
    cfg.listen_bindings = bindings;
    workspace::save_agent_config(&agent_id, &cfg).map_err(|e| e.to_string())?;

    let db = db.inner().clone();
    tauri::async_runtime::spawn(async move {
        subscriptions::reconcile_all(&app, &db).await;
    });
    Ok(())
}

/// Proxy to a plugin's `list_channels` tool. Returns whatever the plugin
/// returned. UI code is expected to render what it understands.
#[tauri::command]
pub async fn plugin_list_channels(
    app: tauri::AppHandle,
    plugin_id: String,
    guild_id: Option<String>,
) -> Result<Value, String> {
    let manager = plugins::from_state(&app);
    let manifest = manager
        .manifest(&plugin_id)
        .ok_or_else(|| format!("plugin '{}' not installed", plugin_id))?;
    if !manager.is_enabled(&plugin_id) {
        return Err(format!("plugin '{}' is disabled", plugin_id));
    }
    if !manifest.tools.iter().any(|t| t.name == "list_channels") {
        return Err(format!(
            "plugin '{}' does not expose a 'list_channels' tool",
            plugin_id
        ));
    }
    let args = match guild_id {
        Some(g) => json!({ "guildId": g }),
        None => json!({}),
    };
    let extra_env = plugins::oauth::build_env_for_subprocess(&manifest);
    let raw = manager
        .runtime
        .call_tool(&manifest, "list_channels", &args, &extra_env)
        .await?;
    Ok(unwrap_mcp_text_payload(raw))
}

/// MCP `tools/call` responses are wrapped as
/// `{ content: [{ type: "text", text: "<json-string>" }], isError: false }`.
/// The UI wants the decoded JSON. If the shape doesn't match, return as-is.
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

/// Return the ids of every installed plugin that declares a listener tool
/// (i.e. has a workflow trigger with `subscription_tool`). Used by the UI to
/// populate the "Bind a channel" provider picker.
#[tauri::command]
pub fn list_trigger_capable_plugins(app: tauri::AppHandle) -> Vec<PluginSummary> {
    let manager = plugins::from_state(&app);
    manager
        .manifests()
        .into_iter()
        .filter(|m| {
            m.workflow
                .triggers
                .iter()
                .any(|t| t.subscription_tool.is_some())
                && manager.is_enabled(&m.id)
        })
        .map(|m| PluginSummary {
            id: m.id,
            name: m.name,
        })
        .collect()
}

#[derive(serde::Serialize)]
pub struct PluginSummary {
    pub id: String,
    pub name: String,
}
