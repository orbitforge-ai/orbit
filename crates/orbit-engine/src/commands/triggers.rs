//! Tauri commands for binding channels to agents and exposing plugin
//! channel-picker data.

use chrono::Utc;
use serde_json::{json, Value};
use ulid::Ulid;

use crate::app_context::AppContext;
use crate::executor::workspace;
use crate::models::channel_binding::ChannelBinding;
use crate::models::trigger_event::{TriggerEventChannel, TriggerEventPayload, TriggerEventUser};
use crate::plugins::oauth;
use crate::triggers::{subscriptions, workflow_spawn};

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
    let repos = app.repos.clone();
    tauri::async_runtime::spawn(async move {
        subscriptions::reconcile_all_for_manager_with_repos(&manager, repos).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn emit_fs_trigger_event(
    app: tauri::State<'_, AppContext>,
    workflow_id: String,
    path: String,
    event_type: String,
    is_dir: bool,
    watched_path: String,
) -> Result<(), String> {
    emit_fs_trigger_event_inner(&app, workflow_id, path, event_type, is_dir, watched_path).await
}

async fn emit_fs_trigger_event_inner(
    app: &AppContext,
    workflow_id: String,
    path: String,
    event_type: String,
    is_dir: bool,
    watched_path: String,
) -> Result<(), String> {
    if !matches!(event_type.as_str(), "created" | "modified" | "deleted") {
        tracing::warn!(
            workflow_id,
            event_type,
            "dropping fs event: unsupported event type"
        );
        return Ok(());
    }

    let workflow = match app.repos.project_workflows().get(&workflow_id).await {
        Ok(workflow) => workflow,
        Err(error) => {
            tracing::warn!(
                workflow_id,
                error = %error,
                "dropping fs event: workflow not found"
            );
            return Ok(());
        }
    };
    if !workflow.enabled || workflow.trigger_kind != "trigger.fs-watch" {
        tracing::warn!(
            workflow_id = workflow.id,
            enabled = workflow.enabled,
            trigger_kind = workflow.trigger_kind,
            "dropping fs event: workflow is not an enabled fs-watch trigger"
        );
        return Ok(());
    }

    if app
        .repos
        .workflow_runs()
        .has_active_for_workflow(&workflow_id)
        .await?
    {
        tracing::debug!(
            workflow_id,
            path,
            event_type,
            "dropping fs event — run already active"
        );
        return Ok(());
    }

    let event = build_fs_trigger_payload(&path, &event_type, is_dir, &watched_path);
    workflow_spawn::spawn_workflow_run(
        app.db.clone(),
        app.repos.clone(),
        app.runtime.clone(),
        workflow_id,
        &event,
    );
    Ok(())
}

fn build_fs_trigger_payload(
    path: &str,
    event_type: &str,
    is_dir: bool,
    watched_path: &str,
) -> TriggerEventPayload {
    TriggerEventPayload {
        event_id: Ulid::new().to_string(),
        plugin_id: "core.fs-watch".to_string(),
        kind: "trigger.fs-watch".to_string(),
        channel: TriggerEventChannel {
            id: path.to_string(),
            thread_id: None,
            name: None,
            workspace_id: None,
        },
        user: TriggerEventUser {
            id: "system".to_string(),
            display_name: None,
            bot: true,
        },
        text: format!("{event_type} {path}"),
        mentions: Vec::new(),
        received_at: Utc::now().to_rfc3339(),
        raw: Some(json!({
            "eventType": event_type,
            "isDir": is_dir,
            "watchedPath": watched_path,
        })),
    }
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
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FsTriggerEventArgs {
        workflow_id: String,
        path: String,
        event_type: String,
        is_dir: bool,
        watched_path: String,
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
        reg.register("emit_fs_trigger_event", |ctx, args| async move {
            let a: FsTriggerEventArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            emit_fs_trigger_event_inner(
                &ctx,
                a.workflow_id,
                a.path,
                a.event_type,
                a.is_dir,
                a.watched_path,
            )
            .await?;
            Ok(serde_json::Value::Null)
        });
    }
}

pub use http::register as register_http;
