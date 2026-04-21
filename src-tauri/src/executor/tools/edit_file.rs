use serde_json::json;
use tracing::warn;

use crate::executor::llm_provider::ToolDefinition;
use crate::executor::skills;

use super::{
    context::ToolExecutionContext,
    helpers::validate_path,
    notebook::{
        delete_cell, insert_cell, is_notebook_path, parse_notebook, replace_cell_source,
        serialize_notebook_pretty,
    },
    ToolHandler,
};

pub struct EditFileTool;

#[async_trait::async_trait]
impl ToolHandler for EditFileTool {
    fn name(&self) -> &'static str {
        "edit_file"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Edit a file by replacing exact text. The old_text must match exactly, including whitespace. Use replace_all to replace every occurrence. For .ipynb files, notebook_action supports cell-level edits.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to the file within the workspace"
                    },
                    "old_text": {
                        "type": "string",
                        "description": "The exact text to find"
                    },
                    "new_text": {
                        "type": "string",
                        "description": "The replacement text. Required for text replacement mode."
                    },
                    "replace_all": {
                        "type": "boolean",
                        "description": "If true, replace every occurrence. Defaults to false."
                    },
                    "notebook_action": {
                        "type": "string",
                        "enum": ["replace_cell", "insert_cell", "delete_cell"],
                        "description": "Notebook cell operation (only for .ipynb files). Use instead of old_text/new_text for notebook mode."
                    },
                    "cell_number": {
                        "type": "integer",
                        "description": "0-based cell index for notebook operations. Required with notebook_action."
                    },
                    "cell_type": {
                        "type": "string",
                        "enum": ["code", "markdown"],
                        "description": "Cell type for insert_cell or replace_cell. Defaults to the existing type for replace_cell."
                    },
                    "cell_source": {
                        "type": "string",
                        "description": "Notebook cell content for insert_cell or replace_cell"
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
            .ok_or("edit_file: missing 'path' field")?;

        let workspace_root = ctx.workspace_root();
        let full_path = validate_path(&workspace_root, path)?;
        if !full_path.is_file() {
            return Err(format!("edit_file: '{}' is not an existing file", path));
        }

        if let Some(notebook_action) = input["notebook_action"].as_str() {
            if !is_notebook_path(path) {
                return Err(
                    "edit_file: notebook_action is only supported for .ipynb files".to_string(),
                );
            }
            let result = edit_notebook(&full_path, path, input, notebook_action)?;
            maybe_mark_path_skill_discovery(ctx, &workspace_root, path, &full_path);
            return Ok(result);
        }

        let old_text = input["old_text"]
            .as_str()
            .ok_or("edit_file: missing 'old_text' field")?;
        let new_text = input["new_text"]
            .as_str()
            .ok_or("edit_file: missing 'new_text' field")?;
        let replace_all = input["replace_all"].as_bool().unwrap_or(false);

        let content = std::fs::read_to_string(&full_path)
            .map_err(|e| format!("failed to read {}: {}", path, e))?;
        let (updated, replaced) = apply_exact_edit(&content, old_text, new_text, replace_all)
            .map_err(|message| format!("edit_file: {} in '{}'", message, path))?;

        std::fs::write(&full_path, updated)
            .map_err(|e| format!("failed to write {}: {}", path, e))?;

        maybe_mark_path_skill_discovery(ctx, &workspace_root, path, &full_path);

        Ok((
            format!("Replaced {} occurrence(s) in '{}'", replaced, path),
            false,
        ))
    }
}

fn apply_exact_edit(
    content: &str,
    old_text: &str,
    new_text: &str,
    replace_all: bool,
) -> Result<(String, usize), String> {
    if old_text.is_empty() {
        return Err("old_text must not be empty".to_string());
    }

    let count = content.matches(old_text).count();
    if count == 0 {
        return Err("old_text not found".to_string());
    }
    if count > 1 && !replace_all {
        return Err(format!(
            "old_text found {} times. Use replace_all:true or provide more exact context",
            count
        ));
    }

    let updated = if replace_all {
        content.replace(old_text, new_text)
    } else {
        content.replacen(old_text, new_text, 1)
    };
    let replaced = if replace_all { count } else { 1 };

    Ok((updated, replaced))
}

fn edit_notebook(
    full_path: &std::path::Path,
    path: &str,
    input: &serde_json::Value,
    notebook_action: &str,
) -> Result<(String, bool), String> {
    let cell_number = input["cell_number"]
        .as_u64()
        .ok_or("edit_file: notebook_action requires 'cell_number'")? as usize;
    let content = std::fs::read_to_string(full_path)
        .map_err(|e| format!("failed to read {}: {}", path, e))?;
    let mut notebook = parse_notebook(&content).map_err(|e| format!("edit_file: {}", e))?;

    match notebook_action {
        "replace_cell" => {
            let cell_source = input["cell_source"]
                .as_str()
                .ok_or("edit_file: replace_cell requires 'cell_source'")?;
            let cell_type = input["cell_type"].as_str();
            replace_cell_source(&mut notebook, cell_number, cell_type, cell_source)
                .map_err(|e| format!("edit_file: {}", e))?;
        }
        "insert_cell" => {
            let cell_source = input["cell_source"]
                .as_str()
                .ok_or("edit_file: insert_cell requires 'cell_source'")?;
            let cell_type = input["cell_type"].as_str().unwrap_or("code");
            insert_cell(&mut notebook, cell_number, cell_type, cell_source)
                .map_err(|e| format!("edit_file: {}", e))?;
        }
        "delete_cell" => {
            delete_cell(&mut notebook, cell_number).map_err(|e| format!("edit_file: {}", e))?;
        }
        other => {
            return Err(format!("edit_file: unknown notebook_action '{}'", other));
        }
    }

    let updated = serialize_notebook_pretty(&notebook).map_err(|e| format!("edit_file: {}", e))?;
    std::fs::write(full_path, updated).map_err(|e| format!("failed to write {}: {}", path, e))?;

    Ok((
        format!("Notebook '{}' updated via {}", path, notebook_action),
        false,
    ))
}

fn maybe_mark_path_skill_discovery(
    ctx: &ToolExecutionContext,
    workspace_root: &std::path::Path,
    path: &str,
    full_path: &std::path::PathBuf,
) {
    if let (Some(db), Some(session_id)) = (&ctx.db, ctx.current_session_id.as_deref()) {
        if let Err(err) = skills::mark_matching_path_skills_discoverable(
            db,
            session_id,
            &ctx.agent_id,
            &ctx.disabled_skills,
            workspace_root,
            std::slice::from_ref(full_path),
        ) {
            warn!(
                session_id = session_id,
                path = path,
                error = %err,
                "failed to update path-scoped skill discovery after edit_file"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::apply_exact_edit;

    #[test]
    fn replaces_single_match() {
        let (updated, replaced) =
            apply_exact_edit("hello world", "world", "orbit", false).expect("edit should work");
        assert_eq!(updated, "hello orbit");
        assert_eq!(replaced, 1);
    }

    #[test]
    fn rejects_ambiguous_single_replace() {
        let result = apply_exact_edit("a b a", "a", "z", false);
        assert!(result.is_err());
    }
}
