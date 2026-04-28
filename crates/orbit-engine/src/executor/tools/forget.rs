use serde_json::json;
use tracing::info;

use crate::executor::llm_provider::ToolDefinition;

use super::{context::ToolExecutionContext, ToolHandler};

pub struct ForgetTool;

#[async_trait::async_trait]
impl ToolHandler for ForgetTool {
    fn name(&self) -> &'static str {
        "forget"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Remove a memory by searching for the best match and deleting it."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Description of the memory to forget"
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
            .ok_or("forget: missing 'query' field")?;

        let client = match &ctx.memory_client {
            Some(c) => c,
            None => return Ok(("Memory service is not available.".to_string(), false)),
        };

        info!(run_id = run_id, query = query, "agent tool: forget");

        let matches = match client
            .search_memories(query, &ctx.memory_user_id, None, 1)
            .await
        {
            Ok(m) => m,
            Err(e) => {
                return Ok((
                    format!("Failed to search for memory to forget: {}", e),
                    false,
                ))
            }
        };

        let Some(top) = matches.into_iter().next() else {
            return Ok(("No matching memory found.".to_string(), false));
        };

        let preview: String = top.text.chars().take(80).collect();
        match client.delete_memory(&top.id).await {
            Ok(()) => Ok((format!("Forgot: \"{}\"", preview), false)),
            Err(e) => Ok((format!("Failed to delete memory: {}", e), false)),
        }
    }
}
