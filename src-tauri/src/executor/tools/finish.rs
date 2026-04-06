use serde_json::json;

use crate::executor::llm_provider::ToolDefinition;

use super::{context::ToolExecutionContext, ToolHandler};

pub struct FinishTool;

#[async_trait::async_trait]
impl ToolHandler for FinishTool {
    fn name(&self) -> &'static str {
        "finish"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Signal that the goal has been completed. Provide a summary of what was accomplished.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "summary": {
                        "type": "string",
                        "description": "A summary of what was accomplished"
                    }
                },
                "required": ["summary"]
            }),
        }
    }

    async fn execute(
        &self,
        _ctx: &ToolExecutionContext,
        input: &serde_json::Value,
        _app: &tauri::AppHandle,
        _run_id: &str,
    ) -> Result<(String, bool), String> {
        let summary = input["summary"]
            .as_str()
            .unwrap_or("Agent finished without summary");
        Ok((summary.to_string(), true))
    }
}
