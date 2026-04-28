use serde::Serialize;
use serde_json::json;

use crate::executor::{llm_provider::ToolDefinition, workspace};

use super::{
    context::ToolExecutionContext,
    session_helpers::{
        duration_seconds, estimate_input_cost_usd, load_owned_session, resolve_session_id,
        session_message_stats,
    },
    ToolHandler,
};

pub struct SessionStatusTool;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionStatusResult {
    session_id: String,
    title: String,
    session_type: String,
    execution_state: Option<String>,
    current_model: String,
    current_context_tokens: u32,
    estimated_cost_usd: Option<f64>,
    estimated_cost_basis: String,
    duration_seconds: Option<i64>,
    message_count: i64,
    user_message_count: i64,
    assistant_message_count: i64,
    compacted_message_count: i64,
    created_at: String,
    updated_at: String,
    last_message_at: Option<String>,
    finish_summary: Option<String>,
    terminal_error: Option<String>,
}

#[async_trait::async_trait]
impl ToolHandler for SessionStatusTool {
    fn name(&self) -> &'static str {
        "session_status"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Show status and resource information for one of this agent's sessions, including execution state, message counts, tracked context tokens, and an estimated cost.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Session ID to inspect. Use 'current' for the active session. Defaults to 'current'."
                    }
                }
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
            .ok_or("session_status: no database available")?;
        let session_id = resolve_session_id(
            ctx.current_session_id.as_deref(),
            input["session_id"].as_str(),
            self.name(),
        )?;

        let session = load_owned_session(db, &ctx.agent_id, &session_id).await?;
        let stats = session_message_stats(db, &session_id).await?;
        let ws_config = workspace::load_agent_config(&ctx.agent_id).unwrap_or_default();
        let current_context_tokens = session.last_input_tokens.unwrap_or(0).max(0) as u32;
        let duration_end = stats
            .last_message_at
            .as_deref()
            .unwrap_or(&session.session.updated_at);

        let status = SessionStatusResult {
            session_id: session.session.id.clone(),
            title: session.session.title.clone(),
            session_type: session.session.session_type.clone(),
            execution_state: session.session.execution_state.clone(),
            current_model: ws_config.model.clone(),
            current_context_tokens,
            estimated_cost_usd: estimate_input_cost_usd(&ws_config.model, current_context_tokens),
            estimated_cost_basis: "tracked current-context input tokens".to_string(),
            duration_seconds: duration_seconds(&session.session.created_at, duration_end),
            message_count: stats.message_count,
            user_message_count: stats.user_message_count,
            assistant_message_count: stats.assistant_message_count,
            compacted_message_count: stats.compacted_message_count,
            created_at: session.session.created_at.clone(),
            updated_at: session.session.updated_at.clone(),
            last_message_at: stats.last_message_at,
            finish_summary: session.session.finish_summary.clone(),
            terminal_error: session.session.terminal_error.clone(),
        };

        let result = serde_json::to_string_pretty(&status)
            .map_err(|e| format!("session_status: failed to serialize result: {}", e))?;
        Ok((result, false))
    }
}
