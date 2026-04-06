use serde_json::json;

use crate::executor::llm_provider::ToolDefinition;

use super::{context::ToolExecutionContext, helpers::validate_path, ToolHandler};

pub struct EditFileTool;

#[async_trait::async_trait]
impl ToolHandler for EditFileTool {
    fn name(&self) -> &'static str {
        "edit_file"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Edit a file by replacing exact text. The old_text must match exactly, including whitespace. Use replace_all to replace every occurrence.".to_string(),
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
                        "description": "The replacement text"
                    },
                    "replace_all": {
                        "type": "boolean",
                        "description": "If true, replace every occurrence. Defaults to false."
                    }
                },
                "required": ["path", "old_text", "new_text"]
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
        let old_text = input["old_text"]
            .as_str()
            .ok_or("edit_file: missing 'old_text' field")?;
        let new_text = input["new_text"]
            .as_str()
            .ok_or("edit_file: missing 'new_text' field")?;
        let replace_all = input["replace_all"].as_bool().unwrap_or(false);

        let workspace_root = ctx.workspace_root();
        let full_path = validate_path(&workspace_root, path)?;
        if !full_path.is_file() {
            return Err(format!("edit_file: '{}' is not an existing file", path));
        }

        let content = std::fs::read_to_string(&full_path)
            .map_err(|e| format!("failed to read {}: {}", path, e))?;
        let (updated, replaced) = apply_exact_edit(&content, old_text, new_text, replace_all)
            .map_err(|message| format!("edit_file: {} in '{}'", message, path))?;

        std::fs::write(&full_path, updated)
            .map_err(|e| format!("failed to write {}: {}", path, e))?;

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
