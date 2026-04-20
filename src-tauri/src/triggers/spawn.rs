//! Spawn an agent run from an inbound trigger event.
//!
//! Reads the same Tauri state the workflow-node agent invoker uses, constructs
//! an `AgentLoopConfig` whose `goal` is the inbound message text, registers a
//! reply target so the agent's `message` tool can reply in-place, then runs
//! the loop on a background task.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};
use tracing::{error, info};
use ulid::Ulid;

use crate::auth::{AuthMode, AuthState};
use crate::commands::users::ActiveUser;
use crate::db::cloud::CloudClientState;
use crate::db::DbPool;
use crate::executor::agent_loop;
use crate::executor::engine::{AgentSemaphores, ExecutorTx, SessionExecutionRegistry};
use crate::executor::permissions::PermissionRegistry;
use crate::executor::workspace;
use crate::memory_service::MemoryServiceState;
use crate::models::channel_binding::ChannelBinding;
use crate::models::task::AgentLoopConfig;
use crate::models::trigger_event::TriggerEventPayload;

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
    let ws_config = workspace::load_agent_config(&agent_id)
        .map_err(|e| format!("load agent config: {}", e))?;
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

    let run_id = format!("trigger-{}", Ulid::new());
    let reply = ReplyChannel {
        plugin_id: event.plugin_id.clone(),
        provider_channel_id: event.channel.id.clone(),
        provider_thread_id: event.channel.thread_id.clone(),
    };
    if let Some(registry) = runtime_app.try_state::<ReplyRegistry>() {
        registry.inner().set(&run_id, reply);
    }

    let goal = build_goal(&event);
    let cfg = AgentLoopConfig {
        goal,
        model: None,
        max_iterations: None,
        max_total_tokens: None,
        template_vars: None,
    };
    let log_path = trigger_agent_log_path(&agent_id, &run_id);

    info!(
        run_id = %run_id,
        agent_id = %agent_id,
        plugin_id = %event.plugin_id,
        channel_id = %event.channel.id,
        "trigger: starting agent run"
    );

    let result = agent_loop::run_agent_loop_for_workflow(
        &run_id,
        &agent_id,
        &cfg,
        &log_path,
        &runtime_app,
        &db,
        &executor_tx,
        None,
        &agent_semaphores,
        &session_registry,
        &permission_registry,
        memory_client.as_ref(),
        &memory_user_id,
        cloud_client,
    )
    .await;

    if let Some(registry) = runtime_app.try_state::<ReplyRegistry>() {
        registry.inner().clear(&run_id);
    }

    match result {
        Ok(outcome) => {
            info!(
                run_id = %run_id,
                iterations = outcome.iterations,
                duration_ms = outcome.duration_ms,
                "trigger: agent run finished"
            );
            Ok(())
        }
        Err(e) => Err(format!("agent loop: {}", e)),
    }
}

/// Build the seed goal given to the agent. Provides enough context for the
/// model to understand who wrote the message and where it came from without
/// forcing the agent into a rigid schema.
fn build_goal(event: &TriggerEventPayload) -> String {
    let speaker = event
        .user
        .display_name
        .clone()
        .unwrap_or_else(|| event.user.id.clone());
    let origin = event
        .channel
        .name
        .as_deref()
        .map(|n| format!("#{}", n))
        .unwrap_or_else(|| format!("channel {}", event.channel.id));
    format!(
        "{speaker} sent a message in {origin}:\n\n{text}\n\n\
         Reply to this message. Use the `message` tool with action='send' \
         and leave `channel` empty — the system routes your reply to the \
         originating {origin}.",
        speaker = speaker,
        origin = origin,
        text = event.text,
    )
}

fn trigger_agent_log_path(agent_id: &str, run_id: &str) -> PathBuf {
    workspace::agent_dir(agent_id)
        .join("trigger_runs")
        .join(format!("{}.log", run_id))
}

async fn resolve_memory_user_id(app: &AppHandle) -> String {
    match app.state::<AuthState>().get().await {
        AuthMode::Cloud(session) => session.user_id,
        _ => app.state::<ActiveUser>().get().await,
    }
}
