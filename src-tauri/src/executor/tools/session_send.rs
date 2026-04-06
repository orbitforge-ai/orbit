use std::cmp;
use std::time::Duration;

use serde::Serialize;
use serde_json::json;

use crate::executor::llm_provider::ToolDefinition;

use super::{
    context::ToolExecutionContext,
    session_control::{
        append_user_text_message, current_bus_run_id, load_accessible_session,
        load_execution_state, record_bus_message, start_session_run, update_session_chain_depth,
        wait_for_session_terminal, wrap_agent_message, MAX_SESSION_CHAIN_DEPTH,
    },
    ToolHandler,
};

const WAIT_TIMEOUT_SECS: u64 = 120;

pub struct SessionSendTool;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionSendResult {
    session_id: String,
    target_agent_id: String,
    trigger_run: bool,
    started_run: bool,
    wait_for_result: bool,
    execution_state: Option<String>,
    finish_summary: Option<String>,
    terminal_error: Option<String>,
    notes: Vec<String>,
}

#[async_trait::async_trait]
impl ToolHandler for SessionSendTool {
    fn name(&self) -> &'static str {
        "session_send"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Send a message into an existing session. Unlike send_message, this appends to an existing conversation and can optionally trigger the session to run again.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "The target session ID."
                    },
                    "message": {
                        "type": "string",
                        "description": "The message to append to the session."
                    },
                    "trigger_run": {
                        "type": "boolean",
                        "description": "If true, trigger the session to run after appending the message. Default: true."
                    },
                    "wait_for_result": {
                        "type": "boolean",
                        "description": "If true and a new run is started, wait for a terminal result before returning. Default: false."
                    }
                },
                "required": ["session_id", "message"]
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
            .ok_or("session_send: no database available")?;
        let session_id = input["session_id"]
            .as_str()
            .ok_or("session_send: missing 'session_id' field")?;
        let message = input["message"]
            .as_str()
            .ok_or("session_send: missing 'message' field")?;
        let trigger_run = input["trigger_run"].as_bool().unwrap_or(true);
        let wait_for_result = input["wait_for_result"].as_bool().unwrap_or(false);

        let target = load_accessible_session(db, &ctx.agent_id, session_id).await?;
        let from_agent_id = ctx.current_agent_id.as_deref().unwrap_or(&ctx.agent_id);
        let is_cross_agent = target.session.agent_id != ctx.agent_id;
        let appended_message = if is_cross_agent {
            wrap_agent_message(from_agent_id, message)
        } else {
            message.to_string()
        };

        append_user_text_message(db, session_id, &appended_message, ctx.cloud_client.clone())
            .await?;

        if is_cross_agent {
            let app = ctx
                .app
                .as_ref()
                .ok_or("session_send: app handle not available")?;
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

        let mut notes = Vec::new();
        let mut started_run = false;
        let mut execution_state = load_execution_state(db, session_id).await?;
        let mut finish_summary = None;
        let mut terminal_error = None;

        if wait_for_result && !trigger_run {
            notes.push(
                "wait_for_result only applies when trigger_run is true and a new run starts."
                    .to_string(),
            );
        }

        if trigger_run {
            if execution_state.as_deref() == Some("running") {
                notes.push(
                    "The session was already running, so no second run was started. The new message will be available for a later run."
                        .to_string(),
                );
            } else {
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
                    target.session.parent_session_id.is_some(),
                    target.allow_sub_agents,
                )
                .await?;
                started_run = true;
                execution_state = Some("running".to_string());
            }
        }

        if wait_for_result && started_run {
            match wait_for_session_terminal(db, session_id, Duration::from_secs(WAIT_TIMEOUT_SECS))
                .await?
            {
                Some(state) => {
                    execution_state = Some(state.execution_state);
                    finish_summary = state.finish_summary;
                    terminal_error = state.terminal_error;
                }
                None => {
                    notes.push(format!(
                        "Timed out waiting after {} seconds. The session may still be running.",
                        WAIT_TIMEOUT_SECS
                    ));
                }
            }
        } else if wait_for_result {
            notes.push("No new run was started, so there was no result to wait for.".to_string());
        }

        let response = SessionSendResult {
            session_id: session_id.to_string(),
            target_agent_id: target.session.agent_id,
            trigger_run,
            started_run,
            wait_for_result,
            execution_state,
            finish_summary,
            terminal_error,
            notes,
        };

        let result = serde_json::to_string_pretty(&response)
            .map_err(|e| format!("session_send: failed to serialize result: {}", e))?;
        Ok((result, false))
    }
}
