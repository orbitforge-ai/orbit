//! Spawn an agent run from an inbound trigger event.
//!
//! Each `(agent, plugin, channel, thread)` tuple gets a long-lived
//! `chat_session` (see `triggers::channel_session`). Inbound messages are
//! saved as user turns in that session, then `session_agent::run_agent_session`
//! loads the full non-compacted history, runs the loop, and appends
//! assistant/tool turns back into the session. Auto-compaction fires
//! inside `run_session_loop` once context usage crosses the configured
//! threshold, so token cost stays bounded as the channel conversation grows.
//!
//! A `ReplyRegistry` entry keyed by the session's stream id (`chat:{session_id}`)
//! lets the `message` tool reply in the same channel without the agent
//! naming it.
use tauri::{AppHandle, Manager};
use tracing::{error, info};

use crate::auth::{AuthMode, AuthState};
use crate::commands::users::ActiveUser;
use crate::db::cloud::CloudClientState;
use crate::db::DbPool;
use crate::executor::engine::{AgentSemaphores, ExecutorTx, SessionExecutionRegistry};
use crate::executor::llm_provider::ContentBlock;
use crate::executor::permissions::PermissionRegistry;
use crate::executor::{session_agent, workspace};
use crate::memory_service::MemoryServiceState;
use crate::models::channel_binding::ChannelBinding;
use crate::models::trigger_event::TriggerEventPayload;
use crate::plugins;

use super::channel_session;
use super::reply_registry::{ReplyChannel, ReplyRegistry};

/// Spawn a fresh agent run on a background task with the event's text as the
/// seed goal. Non-blocking — returns immediately after scheduling.
pub fn spawn_agent_run_from_event(
    app: AppHandle,
    agent_id: String,
    binding: ChannelBinding,
    event: TriggerEventPayload,
) {
    tokio::spawn(async move {
        if let Err(err) = run(app, agent_id, binding, event).await {
            error!(error = %err, "trigger: failed to spawn agent run");
        }
    });
}

async fn run(
    app: AppHandle,
    agent_id: String,
    _binding: ChannelBinding,
    event: TriggerEventPayload,
) -> Result<(), String> {
    let ws_config =
        workspace::load_agent_config(&agent_id).map_err(|e| format!("load agent config: {}", e))?;
    if ws_config.provider.is_empty() {
        return Err(format!("agent {} has no provider configured", agent_id));
    }

    // Reach across to the runtime app handle (matches the workflow node path).
    let runtime_app = app
        .try_state::<crate::RuntimeAppHandleState>()
        .map(|state| state.0.clone())
        .unwrap_or_else(|| app.clone());

    let db = runtime_app.state::<DbPool>().inner().clone();
    let executor_tx = runtime_app.state::<ExecutorTx>().0.clone();
    let agent_semaphores = runtime_app.state::<AgentSemaphores>().inner().clone();
    let session_registry = runtime_app
        .state::<SessionExecutionRegistry>()
        .inner()
        .clone();
    let permission_registry = runtime_app.state::<PermissionRegistry>().inner().clone();
    let memory_client = runtime_app
        .state::<Option<MemoryServiceState>>()
        .as_ref()
        .map(|state| state.client.clone());
    let memory_user_id = resolve_memory_user_id(&runtime_app).await;
    let cloud_client = runtime_app.state::<CloudClientState>().get();

    // ── Resolve the per-channel session and persist the inbound message ──
    let session_id = channel_session::resolve_session_id(&db, &agent_id, &event).await?;
    let user_text = build_user_message(&event);
    session_agent::save_chat_message(
        &db.0,
        &session_id,
        "user",
        &[ContentBlock::Text {
            text: user_text.clone(),
        }],
        cloud_client.clone(),
    )
    .await?;

    // The tool-execution context inside `run_agent_session` uses
    // `stream_id = format!("chat:{session_id}")` as its `run_id`, so the
    // reply registry key must match — that is what the `message` tool reads.
    let stream_id = format!("chat:{}", session_id);
    let reply = ReplyChannel {
        plugin_id: event.plugin_id.clone(),
        provider_channel_id: event.channel.id.clone(),
        provider_thread_id: event.channel.thread_id.clone(),
    };
    if let Some(registry) = runtime_app.try_state::<ReplyRegistry>() {
        registry.inner().set(&stream_id, reply);
    }

    info!(
        session_id = %session_id,
        agent_id = %agent_id,
        plugin_id = %event.plugin_id,
        channel_id = %event.channel.id,
        "trigger: running channel session"
    );

    call_typing_tool(&runtime_app, &event, "start_typing").await;

    let result = session_agent::run_agent_session(
        &agent_id,
        &session_id,
        0,
        false,
        true,
        &db,
        &runtime_app,
        &executor_tx,
        &agent_semaphores,
        &session_registry,
        &permission_registry,
        None,
        memory_client.as_ref(),
        &memory_user_id,
        cloud_client,
    )
    .await;

    if let Some(registry) = runtime_app.try_state::<ReplyRegistry>() {
        registry.inner().clear(&stream_id);
    }

    call_typing_tool(&runtime_app, &event, "stop_typing").await;

    match result {
        Ok(summary) => {
            info!(
                session_id = %session_id,
                summary = %summary,
                "trigger: channel session finished"
            );
            Ok(())
        }
        Err(e) => Err(format!("session run: {}", e)),
    }
}

/// Format the incoming message as the user turn content. Lightweight
/// context preamble (speaker + origin) so the model knows who is talking
/// without needing separate metadata channels — it travels with every
/// inbound message since each Discord user posts as the same `user` role.
fn build_user_message(event: &TriggerEventPayload) -> String {
    let speaker = event
        .user
        .display_name
        .clone()
        .unwrap_or_else(|| event.user.id.clone());
    format!("{speaker}: {text}", speaker = speaker, text = event.text)
}

/// Invoke `start_typing` / `stop_typing` on the originating plugin if it
/// declares the tool. Best-effort — errors are logged but never fail the run.
async fn call_typing_tool(app: &AppHandle, event: &TriggerEventPayload, tool: &str) {
    let manager = plugins::from_state(app);
    let Some(manifest) = manager.manifest(&event.plugin_id) else {
        return;
    };
    if !manifest.tools.iter().any(|t| t.name == tool) {
        return;
    }
    let mut args = serde_json::json!({ "channelId": event.channel.id });
    if let Some(thread_id) = event.channel.thread_id.as_ref() {
        args["threadId"] = serde_json::Value::String(thread_id.clone());
    }
    let extra_env = plugins::oauth::build_env_for_subprocess(&manifest);
    if let Err(err) = manager
        .runtime
        .call_tool(&manifest, tool, &args, &extra_env)
        .await
    {
        tracing::warn!(plugin_id = %event.plugin_id, tool, "typing tool failed: {}", err);
    }
}

async fn resolve_memory_user_id(app: &AppHandle) -> String {
    match app.state::<AuthState>().get().await {
        AuthMode::Cloud(session) => session.user_id,
        _ => app.state::<ActiveUser>().get().await,
    }
}
