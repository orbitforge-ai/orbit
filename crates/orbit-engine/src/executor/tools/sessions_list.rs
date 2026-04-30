use serde::Serialize;
use serde_json::json;

use crate::executor::llm_provider::ToolDefinition;

use super::{context::ToolExecutionContext, session_helpers::list_owned_sessions, ToolHandler};

pub struct SessionsListTool;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionsListItem {
    id: String,
    title: String,
    session_type: String,
    execution_state: Option<String>,
    chain_depth: i64,
    created_at: String,
    updated_at: String,
    last_message_at: Option<String>,
    last_input_tokens: Option<i64>,
    last_message_preview: Option<String>,
}

#[async_trait::async_trait]
impl ToolHandler for SessionsListTool {
    fn name(&self) -> &'static str {
        "sessions_list"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "List this agent's sessions with optional filters for type, execution state, and title search. Returns IDs, titles, state, and last-message previews.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_type": {
                        "type": "string",
                        "enum": ["user_chat", "sub_agent", "bus_message", "pulse"],
                        "description": "Filter by session type."
                    },
                    "state": {
                        "type": "string",
                        "enum": ["queued", "running", "success", "failure", "cancelled", "timed_out"],
                        "description": "Filter by execution state."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum sessions to return. Defaults to 20 and is capped at 100."
                    },
                    "search": {
                        "type": "string",
                        "description": "Search session titles."
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
            .ok_or("sessions_list: no database available")?;
        let limit = input["limit"].as_i64().unwrap_or(20).clamp(1, 100);
        let sessions = list_owned_sessions(
            db,
            &ctx.agent_id,
            input["session_type"].as_str(),
            input["state"].as_str(),
            input["search"].as_str(),
            limit,
        )
        .await?;

        let payload: Vec<SessionsListItem> = sessions
            .into_iter()
            .map(|record| SessionsListItem {
                id: record.session.id,
                title: record.session.title,
                session_type: record.session.session_type,
                execution_state: record.session.execution_state,
                chain_depth: record.session.chain_depth,
                created_at: record.session.created_at,
                updated_at: record.session.updated_at,
                last_message_at: record.last_message_at,
                last_input_tokens: record.last_input_tokens,
                last_message_preview: record.last_message_preview,
            })
            .collect();

        let result = serde_json::to_string_pretty(&payload)
            .map_err(|e| format!("sessions_list: failed to serialize result: {}", e))?;
        Ok((result, false))
    }
}
