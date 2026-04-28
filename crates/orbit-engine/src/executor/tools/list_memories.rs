use serde_json::json;
use tracing::info;

use crate::executor::llm_provider::ToolDefinition;

use super::{context::ToolExecutionContext, ToolHandler};

pub struct ListMemoriesTool;

#[async_trait::async_trait]
impl ToolHandler for ListMemoriesTool {
    fn name(&self) -> &'static str {
        "list_memories"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "List all memories, optionally filtered by type.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "memory_type": {
                        "type": "string",
                        "enum": ["user", "feedback", "project", "reference"],
                        "description": "Optional: filter by memory category"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum results to return (default: 50, max: 200)"
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
        run_id: &str,
    ) -> Result<(String, bool), String> {
        let memory_type = input["memory_type"].as_str();
        let limit = input["limit"].as_u64().unwrap_or(50).min(200) as u32;

        let client = match &ctx.memory_client {
            Some(c) => c,
            None => return Ok(("Memory service is not available.".to_string(), false)),
        };

        info!(run_id = run_id, "agent tool: list_memories");

        match client
            .list_memories(&ctx.memory_user_id, memory_type, limit, 0)
            .await
        {
            Ok(entries) if entries.is_empty() => Ok(("No memories stored.".to_string(), false)),
            Ok(entries) => {
                let lines: Vec<String> = entries
                    .iter()
                    .map(|e| format!("[{}] {} ({})", e.memory_type, e.text, e.created_at))
                    .collect();
                Ok((lines.join("\n"), false))
            }
            Err(e) => Ok((format!("Failed to list memories: {}", e), false)),
        }
    }
}
