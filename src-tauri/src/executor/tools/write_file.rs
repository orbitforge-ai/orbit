use serde_json::json;

use crate::executor::llm_provider::ToolDefinition;

use super::{context::ToolExecutionContext, helpers::validate_path, ToolHandler};

pub struct WriteFileTool;

#[async_trait::async_trait]
impl ToolHandler for WriteFileTool {
    fn name(&self) -> &'static str {
        "write_file"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Write content to a file in the agent's workspace. Creates parent directories if needed. Path is relative to the workspace directory.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to the file within the workspace"
                    },
                    "content": {
                        "type": "string",
                        "description": "The content to write to the file"
                    }
                },
                "required": ["path", "content"]
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
        let path = input["path"]
            .as_str()
            .ok_or("write_file: missing 'path' field")?;
        let content = input["content"]
            .as_str()
            .ok_or("write_file: missing 'content' field")?;

        let workspace_root = ctx.workspace_root();
        let full_path = validate_path(&workspace_root, path)?;
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("failed to create dirs: {}", e))?;
        }
        std::fs::write(&full_path, content)
            .map_err(|e| format!("failed to write {}: {}", path, e))?;

        Ok((
            format!("Successfully wrote {} bytes to {}", content.len(), path),
            false,
        ))
    }
}
