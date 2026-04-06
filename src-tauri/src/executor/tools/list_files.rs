use serde_json::json;

use crate::executor::llm_provider::ToolDefinition;

use super::{context::ToolExecutionContext, helpers::validate_path, ToolHandler};

pub struct ListFilesTool;

#[async_trait::async_trait]
impl ToolHandler for ListFilesTool {
    fn name(&self) -> &'static str {
        "list_files"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "List files and directories in the agent's workspace. Path is relative to the workspace directory.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to the directory within the workspace. Use '.' for the workspace root."
                    }
                },
                "required": ["path"]
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
            .ok_or("list_files: missing 'path' field")?;

        let full_path = validate_path(&ctx.workspace_root, path)?;
        if !full_path.is_dir() {
            return Err(format!("{} is not a directory", path));
        }

        let entries =
            std::fs::read_dir(&full_path).map_err(|e| format!("failed to list {}: {}", path, e))?;

        let mut listing = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let meta = entry.metadata().map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().to_string();
            let kind = if meta.is_dir() { "dir" } else { "file" };
            let size = meta.len();
            listing.push(format!("{:>6} {:4} {}", size, kind, name));
        }
        listing.sort();

        if listing.is_empty() {
            Ok(("(empty directory)".to_string(), false))
        } else {
            Ok((listing.join("\n"), false))
        }
    }
}
