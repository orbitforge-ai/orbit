use std::time::Duration;

use serde::Serialize;
use serde_json::json;

use crate::executor::{llm_provider::ToolDefinition, session_agent};

use super::{
    context::ToolExecutionContext,
    session_control::{
        create_session_with_initial_message, current_bus_run_id, record_bus_message, resolve_agent,
        start_session_run, update_session_source_bus_message, wait_for_session_terminal,
        wrap_agent_message, MAX_SESSION_CHAIN_DEPTH,
    },
    ToolHandler,
};

const DEFAULT_TIMEOUT_SECS: u64 = 300;
const MAX_TIMEOUT_SECS: u64 = 600;

pub struct SessionsSpawnTool;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionsSpawnResult {
    session_id: String,
    agent_id: String,
    agent_name: String,
    mode: String,
    session_type: String,
    allow_sub_agents: bool,
    execution_state: String,
    finish_summary: Option<String>,
    terminal_error: Option<String>,
}

#[async_trait::async_trait]
impl ToolHandler for SessionsSpawnTool {
    fn name(&self) -> &'static str {
        "sessions_spawn"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Spawn an isolated session with configurable mode. mode='run' executes once and waits for a result, while mode='session' creates a persistent session you can resume later with session_send.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "goal": {
                        "type": "string",
                        "description": "The initial goal or message for the spawned session."
                    },
                    "mode": {
                        "type": "string",
                        "enum": ["run", "session"],
                        "description": "'run' executes once and waits for a result. 'session' starts a persistent session."
                    },
                    "agent": {
                        "type": "string",
                        "description": "Optional target agent name or ID. Defaults to the current agent."
                    },
                    "label": {
                        "type": "string",
                        "description": "Optional label/title for the spawned session."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "description": "Timeout for run mode in seconds. Defaults to 300 and is capped at 600."
                    },
                    "allow_sub_agents": {
                        "type": "boolean",
                        "description": "Whether the spawned session may use spawn_sub_agents on later runs. Default: false."
                    }
                },
                "required": ["goal"]
            }),
        }
    }

    async fn execute(
        &self,
        ctx: &ToolExecutionContext,
        input: &serde_json::Value,
        _app: &tauri::AppHandle,
        _run_id: &str,
    ) -> Result<(String, bool), String> {
        let db = ctx
            .db
            .as_ref()
            .ok_or("sessions_spawn: no database available")?;
        let goal = input["goal"]
            .as_str()
            .ok_or("sessions_spawn: missing 'goal' field")?;
        let mode = input["mode"].as_str().unwrap_or("run");
        if !matches!(mode, "run" | "session") {
            return Err(format!("sessions_spawn: unsupported mode '{}'", mode));
        }

        let timeout_secs = input["timeout_seconds"]
            .as_u64()
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .min(MAX_TIMEOUT_SECS);
        let allow_sub_agents = input["allow_sub_agents"].as_bool().unwrap_or(false);
        let from_agent_id = ctx.current_agent_id.as_deref().unwrap_or(&ctx.agent_id);
        let next_depth = ctx.chain_depth + 1;
        if next_depth > MAX_SESSION_CHAIN_DEPTH {
            return Ok((
                format!(
                    "Error: Maximum chain depth ({}) exceeded. Cannot spawn further sessions to prevent infinite loops.",
                    MAX_SESSION_CHAIN_DEPTH
                ),
                false,
            ));
        }

        let target = resolve_agent(db, input["agent"].as_str(), &ctx.agent_id).await?;
        let is_cross_agent = target.id != ctx.agent_id;
        let session_type = if is_cross_agent {
            "bus_message"
        } else {
            "sub_agent"
        };
        let initial_goal = if is_cross_agent {
            wrap_agent_message(from_agent_id, goal)
        } else {
            goal.to_string()
        };

        let session_id = create_session_with_initial_message(
            db,
            &target.id,
            session_type,
            input["label"].as_str(),
            ctx.current_session_id.as_deref(),
            next_depth,
            allow_sub_agents,
            &initial_goal,
            None,
        )
        .await?;

        if is_cross_agent {
            let app = ctx
                .app
                .as_ref()
                .ok_or("sessions_spawn: app handle not available")?;
            let bus_message_id = record_bus_message(
                db,
                app,
                from_agent_id,
                current_bus_run_id(ctx).as_deref(),
                ctx.current_session_id.as_deref(),
                &target.id,
                Some(&session_id),
                goal,
            )
            .await?;
            update_session_source_bus_message(db, &session_id, &bus_message_id).await?;
        }

        let is_sub_agent = ctx.current_session_id.is_some();
        start_session_run(
            ctx,
            &target.id,
            &session_id,
            next_depth,
            is_sub_agent,
            allow_sub_agents,
        )
        .await?;

        if mode == "session" {
            let response = SessionsSpawnResult {
                session_id,
                agent_id: target.id,
                agent_name: target.name,
                mode: mode.to_string(),
                session_type: session_type.to_string(),
                allow_sub_agents,
                execution_state: "running".to_string(),
                finish_summary: None,
                terminal_error: None,
            };
            let result = serde_json::to_string_pretty(&response)
                .map_err(|e| format!("sessions_spawn: failed to serialize result: {}", e))?;
            return Ok((result, false));
        }

        let terminal =
            wait_for_session_terminal(db, &session_id, Duration::from_secs(timeout_secs)).await?;

        let response = match terminal {
            Some(state) => SessionsSpawnResult {
                session_id,
                agent_id: target.id,
                agent_name: target.name,
                mode: mode.to_string(),
                session_type: session_type.to_string(),
                allow_sub_agents,
                execution_state: state.execution_state,
                finish_summary: state.finish_summary,
                terminal_error: state.terminal_error,
            },
            None => {
                let registry = ctx
                    .session_registry
                    .as_ref()
                    .ok_or("sessions_spawn: session registry not available")?;
                registry.cancel(&session_id).await;
                session_agent::update_session_execution_state(
                    db,
                    &session_id,
                    "timed_out",
                    None,
                    Some(format!("Session timed out after {} seconds.", timeout_secs)),
                )
                .await?;
                SessionsSpawnResult {
                    session_id,
                    agent_id: target.id,
                    agent_name: target.name,
                    mode: mode.to_string(),
                    session_type: session_type.to_string(),
                    allow_sub_agents,
                    execution_state: "timed_out".to_string(),
                    finish_summary: None,
                    terminal_error: Some(format!(
                        "Session timed out after {} seconds.",
                        timeout_secs
                    )),
                }
            }
        };

        let result = serde_json::to_string_pretty(&response)
            .map_err(|e| format!("sessions_spawn: failed to serialize result: {}", e))?;
        Ok((result, false))
    }
}
