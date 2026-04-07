use serde_json::json;

use crate::executor::llm_provider::ToolDefinition;

use super::{
    context::ToolExecutionContext,
    helpers::validate_path,
    notebook::{is_notebook_path, notebook_from_input, serialize_notebook_pretty},
    ToolHandler,
};

pub struct WriteFileTool;

#[async_trait::async_trait]
impl ToolHandler for WriteFileTool {
    fn name(&self) -> &'static str {
        "write_file"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Write content to a file in the agent's workspace. Creates parent directories if needed. Path is relative to the workspace directory. For .ipynb files, content may be raw notebook JSON or a structured notebook object.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to the file within the workspace"
                    },
                    "content": {
                        "description": "The content to write. For .ipynb files, this may be a JSON string or notebook object."
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
        let content_value = input
            .get("content")
            .ok_or("write_file: missing 'content' field")?;

        let workspace_root = ctx.workspace_root();
        let full_path = validate_path(&workspace_root, path)?;
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("failed to create dirs: {}", e))?;
        }
        let content = if is_notebook_path(path) {
            let notebook =
                notebook_from_input(content_value).map_err(|e| format!("write_file: {}", e))?;
            serialize_notebook_pretty(&notebook).map_err(|e| format!("write_file: {}", e))?
        } else {
            content_value
                .as_str()
                .ok_or("write_file: content must be a string for non-notebook files")?
                .to_string()
        };

        std::fs::write(&full_path, &content)
            .map_err(|e| format!("failed to write {}: {}", path, e))?;

        Ok((
            format!("Successfully wrote {} bytes to {}", content.len(), path),
            false,
        ))
    }
}
