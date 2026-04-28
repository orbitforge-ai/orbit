use serde_json::json;

use crate::executor::llm_provider::ToolDefinition;

use super::{
    context::ToolExecutionContext,
    session_helpers::{list_session_messages, load_owned_session, resolve_session_id},
    ToolHandler,
};

pub struct SessionHistoryTool;

#[async_trait::async_trait]
impl ToolHandler for SessionHistoryTool {
    fn name(&self) -> &'static str {
        "session_history"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Fetch message history for one of this agent's sessions. Useful for reviewing past conversations or gathering context from related sessions.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "The session ID to inspect. Use 'current' for the active session."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum messages to return. Defaults to 50 and is capped at 200."
                    },
                    "offset": {
                        "type": "integer",
                        "description": "How many earlier messages to skip."
                    }
                },
                "required": ["session_id"]
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
            .ok_or("session_history: no database available")?;
        let session_id = resolve_session_id(
            ctx.current_session_id.as_deref(),
            input["session_id"].as_str(),
            self.name(),
        )?;
        let limit = input["limit"].as_i64().unwrap_or(50).clamp(1, 200);
        let offset = input["offset"].as_i64().unwrap_or(0).max(0);

        load_owned_session(db, &ctx.agent_id, &session_id).await?;
        let messages = list_session_messages(db, &session_id, limit, offset).await?;

        let result = serde_json::to_string_pretty(&messages)
            .map_err(|e| format!("session_history: failed to serialize result: {}", e))?;
        Ok((result, false))
    }
}
