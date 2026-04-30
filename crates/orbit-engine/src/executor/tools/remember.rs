use serde_json::json;
use tracing::info;

use crate::executor::llm_provider::ToolDefinition;

use super::{context::ToolExecutionContext, ToolHandler};

pub struct RememberTool;

#[async_trait::async_trait]
impl ToolHandler for RememberTool {
    fn name(&self) -> &'static str {
        "remember"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Save a piece of information to long-term memory. Use this to persist important facts, user preferences, feedback, or project context across sessions.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The information to remember"
                    },
                    "memory_type": {
                        "type": "string",
                        "enum": ["user", "feedback", "project", "reference"],
                        "description": "Category: 'user' for user facts/preferences, 'feedback' for guidance on your approach, 'project' for project context/decisions, 'reference' for pointers to external resources"
                    }
                },
                "required": ["text", "memory_type"]
            }),
        }
    }

    async fn execute(
        &self,
        ctx: &ToolExecutionContext,
        input: &serde_json::Value,
        _app: &tauri::AppHandle,
        run_id: &str,
    ) -> Result<(String, bool), String> {
        let text = input["text"]
            .as_str()
            .ok_or("remember: missing 'text' field")?;
        let memory_type = input["memory_type"]
            .as_str()
            .ok_or("remember: missing 'memory_type' field")?;

        if !matches!(memory_type, "user" | "feedback" | "project" | "reference") {
            return Ok((
                format!(
                    "Error: invalid memory_type '{}'. Must be one of: user, feedback, project, reference",
                    memory_type
                ),
                false,
            ));
        }

        let client = match &ctx.memory_client {
            Some(c) => c,
            None => return Ok(("Memory service is not available.".to_string(), false)),
        };

        info!(
            run_id = run_id,
            memory_type = memory_type,
            "agent tool: remember"
        );

        match client
            .add_memory(text, memory_type, &ctx.memory_user_id, None)
            .await
        {
            Ok(_) => Ok((
                format!("Remembered: \"{}\" (type: {})", text, memory_type),
                false,
            )),
            Err(e) => Ok((format!("Failed to save memory: {}", e), false)),
        }
    }
}
