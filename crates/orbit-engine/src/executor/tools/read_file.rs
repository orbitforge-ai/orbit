use serde_json::json;
use tracing::warn;

use crate::executor::llm_provider::ToolDefinition;
use crate::executor::skills;

use super::{
    context::ToolExecutionContext,
    helpers::validate_path,
    notebook::{format_notebook, is_notebook_path, parse_notebook},
    ToolHandler,
};

pub struct ReadFileTool;

#[async_trait::async_trait]
impl ToolHandler for ReadFileTool {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Read the contents of a file from the agent's workspace. Path is relative to the workspace directory. Jupyter notebooks (.ipynb) are rendered as human-readable cells and outputs.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to the file within the workspace"
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
            .ok_or("read_file: missing 'path' field")?;

        let workspace_root = ctx.workspace_root();
        let full_path = validate_path(&workspace_root, path)?;
        let content = std::fs::read_to_string(&full_path)
            .map_err(|e| format!("failed to read {}: {}", path, e))?;

        if let (Some(db), Some(session_id)) = (&ctx.db, ctx.current_session_id.as_deref()) {
            if let Err(err) = skills::mark_matching_path_skills_discoverable(
                db,
                session_id,
                &ctx.agent_id,
                &ctx.disabled_skills,
                &workspace_root,
                std::slice::from_ref(&full_path),
            ) {
                warn!(
                    session_id = session_id,
                    path = path,
                    error = %err,
                    "failed to update path-scoped skill discovery after read_file"
                );
            }
        }

        if is_notebook_path(path) {
            let notebook = parse_notebook(&content).map_err(|e| format!("read_file: {}", e))?;
            return Ok((format_notebook(&notebook), false));
        }

        let content = if content.len() > 100_000 {
            let mut truncated = content[..100_000].to_string();
            truncated.push_str("\n[file truncated at 100KB]");
            truncated
        } else {
            content
        };

        Ok((content, false))
    }
}
