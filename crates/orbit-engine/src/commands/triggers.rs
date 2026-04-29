//! Tauri commands for binding channels to agents and exposing plugin
//! channel-picker data.

use serde_json::{json, Value};

use crate::app_context::AppContext;
use crate::executor::workspace;
use crate::models::channel_binding::ChannelBinding;
use crate::plugins::oauth;
use crate::triggers::subscriptions;

#[tauri::command]
pub async fn list_agent_listen_bindings(agent_id: String) -> Result<Vec<ChannelBinding>, String> {
    let cfg = workspace::load_agent_config(&agent_id).map_err(|e| e.to_string())?;
    Ok(cfg.listen_bindings)
}

#[tauri::command]
pub async fn set_agent_listen_bindings(
    app: tauri::State<'_, AppContext>,
    agent_id: String,
    bindings: Vec<ChannelBinding>,
) -> Result<(), String> {
    set_agent_listen_bindings_inner(&app, agent_id, bindings).await
}

async fn set_agent_listen_bindings_inner(
    app: &AppContext,
    agent_id: String,
    bindings: Vec<ChannelBinding>,
) -> Result<(), String> {
    let mut cfg = workspace::load_agent_config(&agent_id).map_err(|e| e.to_string())?;
    cfg.listen_bindings = bindings;
    workspace::save_agent_config(&agent_id, &cfg).map_err(|e| e.to_string())?;

    let manager = app.plugins.clone();
    let db = app.db.clone();
    tauri::async_runtime::spawn(async move {
        subscriptions::reconcile_all_for_manager(&manager, &db).await;
    });
    Ok(())
}

/// Proxy to a plugin's `list_channels` tool. Returns whatever the plugin
/// returned. UI code is expected to render what it understands.
#[tauri::command]
pub async fn plugin_list_channels(
    app: tauri::State<'_, AppContext>,
    plugin_id: String,
    guild_id: Option<String>,
) -> Result<Value, String> {
    plugin_list_channels_inner(&app, plugin_id, guild_id).await
}

async fn plugin_list_channels_inner(
    app: &AppContext,
    plugin_id: String,
    guild_id: Option<String>,
) -> Result<Value, String> {
    let manager = &app.plugins;
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
    let extra_env = oauth::build_env_for_subprocess(&manifest);
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
pub fn list_trigger_capable_plugins(app: tauri::State<'_, AppContext>) -> Vec<PluginSummary> {
    list_trigger_capable_plugins_inner(&app)
}

fn list_trigger_capable_plugins_inner(app: &AppContext) -> Vec<PluginSummary> {
    let manager = &app.plugins;
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

mod http {
    use super::*;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct AgentIdArgs {
        agent_id: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SetBindingsArgs {
        agent_id: String,
        bindings: Vec<ChannelBinding>,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct PluginChannelsArgs {
        plugin_id: String,
        #[serde(default)]
        guild_id: Option<String>,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("list_agent_listen_bindings", |_ctx, args| async move {
            let a: AgentIdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = list_agent_listen_bindings(a.agent_id).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("set_agent_listen_bindings", |ctx, args| async move {
            let a: SetBindingsArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            set_agent_listen_bindings_inner(&ctx, a.agent_id, a.bindings).await?;
            Ok(serde_json::Value::Null)
        });
        reg.register("plugin_list_channels", |ctx, args| async move {
            let a: PluginChannelsArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = plugin_list_channels_inner(&ctx, a.plugin_id, a.guild_id).await?;
            Ok(r)
        });
        reg.register("list_trigger_capable_plugins", |ctx, _args| async move {
            let r = list_trigger_capable_plugins_inner(&ctx);
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
    }
}

pub use http::register as register_http;
