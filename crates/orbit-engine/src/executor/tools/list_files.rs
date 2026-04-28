use std::time::SystemTime;

use serde_json::json;
use walkdir::WalkDir;

use crate::executor::llm_provider::ToolDefinition;

use super::{
    context::ToolExecutionContext,
    helpers::{compile_globs, matches_globs, validate_path},
    ToolHandler,
};

pub struct ListFilesTool;

const MAX_GLOB_RESULTS: usize = 500;

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
                    },
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern to match files (for example '**/*.rs' or 'src/**/*.tsx'). When provided, searches recursively from path."
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

        let workspace_root = ctx.workspace_root();
        let full_path = validate_path(&workspace_root, path)?;
        if !full_path.is_dir() {
            return Err(format!("{} is not a directory", path));
        }

        if let Some(pattern) = input["pattern"].as_str() {
            return list_matching_files(&workspace_root, &full_path, pattern);
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

fn list_matching_files(
    workspace_root: &std::path::Path,
    search_root: &std::path::Path,
    pattern: &str,
) -> Result<(String, bool), String> {
    let globs = compile_globs(pattern)?;
    let workspace_root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());

    let mut matches: Vec<(String, SystemTime)> = Vec::new();
    for entry in WalkDir::new(search_root)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if !matches_globs(entry.path(), search_root, &globs) {
            continue;
        }

        let modified = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let relative = entry
            .path()
            .strip_prefix(&workspace_root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .to_string();
        matches.push((relative, modified));
    }

    matches.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let truncated = matches.len() > MAX_GLOB_RESULTS;
    matches.truncate(MAX_GLOB_RESULTS);

    if matches.is_empty() {
        return Ok(("(no matches)".to_string(), false));
    }

    let mut lines: Vec<String> = matches.into_iter().map(|(path, _)| path).collect();
    if truncated {
        lines.push(format!(
            "[truncated to {} matches; refine the pattern for more specific results]",
            MAX_GLOB_RESULTS
        ));
    }

    Ok((lines.join("\n"), false))
}
