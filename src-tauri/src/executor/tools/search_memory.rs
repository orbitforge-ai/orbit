use serde_json::json;
use tracing::info;

use crate::executor::llm_provider::ToolDefinition;

use super::{context::ToolExecutionContext, ToolHandler};

pub struct SearchMemoryTool;

#[async_trait::async_trait]
impl ToolHandler for SearchMemoryTool {
    fn name(&self) -> &'static str {
        "search_memory"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description:
                "Search long-term memory for relevant information using semantic similarity."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "What to search for"
                    },
                    "memory_type": {
                        "type": "string",
                        "enum": ["user", "feedback", "project", "reference"],
                        "description": "Optional: filter by memory category"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum results to return (default: 5, max: 20)"
                    }
                },
                "required": ["query"]
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
        let query = input["query"]
            .as_str()
            .ok_or("search_memory: missing 'query' field")?;
        let memory_type = input["memory_type"].as_str();
        let limit = input["limit"].as_u64().unwrap_or(5).min(20) as u32;

        let client = match &ctx.memory_client {
            Some(c) => c,
            None => return Ok(("Memory service is not available.".to_string(), false)),
        };

        info!(run_id = run_id, query = query, "agent tool: search_memory");

        match client
            .search_memories(query, &ctx.memory_user_id, memory_type, limit)
            .await
        {
            Ok(entries) if entries.is_empty() => {
                Ok(("No matching memories found.".to_string(), false))
            }
            Ok(entries) => {
                let lines: Vec<String> = entries
                    .iter()
                    .map(|e| format!("[{}] {} ({})", e.memory_type, e.text, e.created_at))
                    .collect();
                Ok((lines.join("\n"), false))
            }
            Err(e) => Ok((format!("Memory search failed: {}", e), false)),
        }
    }
}
