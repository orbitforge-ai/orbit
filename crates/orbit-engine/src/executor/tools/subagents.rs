use std::cmp;

use serde_json::json;

use crate::executor::{llm_provider::ToolDefinition, session_agent};

use super::{
    context::ToolExecutionContext,
    session_control::{
        append_user_text_message, current_bus_run_id, is_active_state, list_child_sessions,
        load_accessible_session, load_execution_state, record_bus_message, start_session_run,
        update_session_chain_depth, wrap_agent_message, MAX_SESSION_CHAIN_DEPTH,
    },
    ToolHandler,
};

pub struct SubagentsTool;

#[async_trait::async_trait]
impl ToolHandler for SubagentsTool {
    fn name(&self) -> &'static str {
        "subagents"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "List, kill, or steer sessions spawned from the current session. Use this to manage in-flight sub-agent work after it has been created.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "kill", "steer"],
                        "description": "Action to perform."
                    },
                    "session_id": {
                        "type": "string",
                        "description": "Spawned session ID, required for kill and steer."
                    },
                    "message": {
                        "type": "string",
                        "description": "New instructions for the child session, required for steer."
                    }
                },
                "required": ["action"]
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
        let db = ctx.db.as_ref().ok_or("subagents: no database available")?;
        let current_session_id = ctx
            .current_session_id
            .as_deref()
            .ok_or("subagents: no current session available")?;
        let action = input["action"]
            .as_str()
            .ok_or("subagents: missing 'action' field")?;

        match action {
            "list" => {
                let children = list_child_sessions(db, current_session_id).await?;
                let result = serde_json::to_string_pretty(&children)
                    .map_err(|e| format!("subagents: failed to serialize result: {}", e))?;
                Ok((result, false))
            }
            "kill" => {
                let session_id = input["session_id"]
                    .as_str()
                    .ok_or("subagents: kill requires 'session_id'")?;
                let target = load_accessible_session(db, &ctx.agent_id, session_id).await?;
                if target.session.parent_session_id.as_deref() != Some(current_session_id) {
                    return Err(format!(
                        "subagents: session '{}' is not a child of the current session",
                        session_id
                    ));
                }

                let registry = ctx
                    .session_registry
                    .as_ref()
                    .ok_or("subagents: session registry not available")?;
                registry.cancel(session_id).await;
                session_agent::update_session_execution_state(
                    db,
                    session_id,
                    "cancelled",
                    None,
                    Some("Cancelled by parent session.".to_string()),
                )
                .await?;
                Ok((
                    format!("Sub-agent session '{}' has been cancelled.", session_id),
                    false,
                ))
            }
            "steer" => {
                let session_id = input["session_id"]
                    .as_str()
                    .ok_or("subagents: steer requires 'session_id'")?;
                let message = input["message"]
                    .as_str()
                    .ok_or("subagents: steer requires 'message'")?;
                let target = load_accessible_session(db, &ctx.agent_id, session_id).await?;
                if target.session.parent_session_id.as_deref() != Some(current_session_id) {
                    return Err(format!(
                        "subagents: session '{}' is not a child of the current session",
                        session_id
                    ));
                }

                let from_agent_id = ctx.current_agent_id.as_deref().unwrap_or(&ctx.agent_id);
                let appended = wrap_agent_message(from_agent_id, message);
                append_user_text_message(db, session_id, &appended, ctx.cloud_client.clone())
                    .await?;

                if target.session.agent_id != ctx.agent_id {
                    let app = ctx
                        .app
                        .as_ref()
                        .ok_or("subagents: app handle not available")?;
                    record_bus_message(
                        db,
                        app,
                        from_agent_id,
                        current_bus_run_id(ctx).as_deref(),
                        ctx.current_session_id.as_deref(),
                        &target.session.agent_id,
                        Some(session_id),
                        message,
                    )
                    .await?;
                }

                let execution_state = load_execution_state(db, session_id).await?;
                if execution_state
                    .as_deref()
                    .map(is_active_state)
                    .unwrap_or(false)
                {
                    let payload = json!({
                        "sessionId": session_id,
                        "startedRun": false,
                        "executionState": execution_state,
                        "note": "The child session is already running, so the new instructions were queued for a later run."
                    });
                    return Ok((
                        serde_json::to_string_pretty(&payload)
                            .map_err(|e| format!("subagents: failed to serialize result: {}", e))?,
                        false,
                    ));
                }

                let next_depth = cmp::max(ctx.chain_depth, target.session.chain_depth) + 1;
                if next_depth > MAX_SESSION_CHAIN_DEPTH {
                    return Ok((
                        format!(
                            "Error: Maximum chain depth ({}) exceeded. Cannot trigger further sessions to prevent infinite loops.",
                            MAX_SESSION_CHAIN_DEPTH
                        ),
                        false,
                    ));
                }

                update_session_chain_depth(db, session_id, next_depth).await?;
                start_session_run(
                    ctx,
                    &target.session.agent_id,
                    session_id,
                    next_depth,
                    true,
                    target.allow_sub_agents,
                )
                .await?;

                let payload = json!({
                    "sessionId": session_id,
                    "startedRun": true,
                    "executionState": "running",
                    "allowSubAgents": target.allow_sub_agents,
                });
                Ok((
                    serde_json::to_string_pretty(&payload)
                        .map_err(|e| format!("subagents: failed to serialize result: {}", e))?,
                    false,
                ))
            }
            _ => Err(format!("subagents: unsupported action '{}'", action)),
        }
    }
}
