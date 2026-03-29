use serde_json::json;
use std::path::{Path, PathBuf};
use tracing::info;

use crate::events::emitter::emit_log_chunk;
use crate::executor::llm_provider::ToolDefinition;

/// Context for executing agent tools — provides sandboxed filesystem access.
pub struct ToolExecutionContext {
    /// The agent's entire root directory (~/.orbit/agents/{agent_id}/).
    pub agent_root: PathBuf,
    /// The workspace subdirectory for scratch files.
    pub workspace_root: PathBuf,
}

impl ToolExecutionContext {
    pub fn new(agent_id: &str) -> Self {
        let agent_root = super::workspace::agent_dir(agent_id);
        let workspace_root = agent_root.join("workspace");
        Self {
            agent_root,
            workspace_root,
        }
    }
}

/// Build the tool definitions that are exposed to the LLM.
pub fn build_tool_definitions(allowed: &[String]) -> Vec<ToolDefinition> {
    let all_tools = vec![
        ToolDefinition {
            name: "shell_command".to_string(),
            description: "Execute a shell command in the agent's workspace directory. Returns stdout and stderr.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    }
                },
                "required": ["command"]
            }),
        },
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read the contents of a file from the agent's workspace. Path is relative to the workspace directory.".to_string(),
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
        },
        ToolDefinition {
            name: "write_file".to_string(),
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
        },
        ToolDefinition {
            name: "list_files".to_string(),
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
        },
        ToolDefinition {
            name: "finish".to_string(),
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
        },
    ];

    if allowed.is_empty() {
        return all_tools;
    }

    all_tools
        .into_iter()
        .filter(|t| allowed.contains(&t.name))
        .collect()
}

/// Validate a path stays within the given base directory.
fn validate_path(base: &Path, requested: &str) -> Result<PathBuf, String> {
    let resolved = base.join(requested);

    if resolved.exists() {
        let canonical = resolved
            .canonicalize()
            .map_err(|e| format!("failed to resolve path: {}", e))?;
        let base_canonical = base
            .canonicalize()
            .map_err(|e| format!("failed to resolve base: {}", e))?;
        if !canonical.starts_with(&base_canonical) {
            return Err(format!("path escapes workspace: {}", requested));
        }
        return Ok(canonical);
    }

    // For new files, validate the parent
    let parent = resolved.parent().ok_or("invalid path")?;
    if !parent.exists() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create directories: {}", e))?;
    }
    let parent_canonical = parent
        .canonicalize()
        .map_err(|e| format!("failed to resolve parent: {}", e))?;
    let base_canonical = base
        .canonicalize()
        .map_err(|e| format!("failed to resolve base: {}", e))?;
    if !parent_canonical.starts_with(&base_canonical) {
        return Err(format!("path escapes workspace: {}", requested));
    }

    Ok(parent_canonical.join(resolved.file_name().ok_or("no filename")?))
}

/// Execute a single tool call. Returns (result_text, is_finish).
pub async fn execute_tool(
    ctx: &ToolExecutionContext,
    tool_name: &str,
    input: &serde_json::Value,
    app: &tauri::AppHandle,
    run_id: &str,
) -> Result<(String, bool), String> {
    match tool_name {
        "shell_command" => {
            let command = input["command"]
                .as_str()
                .ok_or("shell_command: missing 'command' field")?;

            info!(run_id = run_id, command = command, "agent tool: shell_command");

            // Ensure workspace dir exists
            std::fs::create_dir_all(&ctx.workspace_root)
                .map_err(|e| format!("failed to create workspace: {}", e))?;

            let output = tokio::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(command)
                .current_dir(&ctx.workspace_root)
                .output()
                .await
                .map_err(|e| format!("failed to execute command: {}", e))?;

            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);

            // Emit log chunks
            if !stdout.is_empty() {
                let lines: Vec<(String, String)> = stdout
                    .lines()
                    .map(|l| ("stdout".to_string(), l.to_string()))
                    .collect();
                emit_log_chunk(app, run_id, lines);
            }
            if !stderr.is_empty() {
                let lines: Vec<(String, String)> = stderr
                    .lines()
                    .map(|l| ("stderr".to_string(), l.to_string()))
                    .collect();
                emit_log_chunk(app, run_id, lines);
            }

            let mut result = String::new();
            if !stdout.is_empty() {
                result.push_str(&stdout);
            }
            if !stderr.is_empty() {
                if !result.is_empty() {
                    result.push('\n');
                }
                result.push_str("[stderr]\n");
                result.push_str(&stderr);
            }
            result.push_str(&format!("\n[exit code: {}]", exit_code));

            // Truncate to 50KB to avoid blowing up context
            if result.len() > 50_000 {
                result.truncate(50_000);
                result.push_str("\n[output truncated]");
            }

            Ok((result, false))
        }

        "read_file" => {
            let path = input["path"]
                .as_str()
                .ok_or("read_file: missing 'path' field")?;

            let full_path = validate_path(&ctx.workspace_root, path)?;
            let content = std::fs::read_to_string(&full_path)
                .map_err(|e| format!("failed to read {}: {}", path, e))?;

            // Truncate to 100KB
            let content = if content.len() > 100_000 {
                let mut truncated = content[..100_000].to_string();
                truncated.push_str("\n[file truncated at 100KB]");
                truncated
            } else {
                content
            };

            Ok((content, false))
        }

        "write_file" => {
            let path = input["path"]
                .as_str()
                .ok_or("write_file: missing 'path' field")?;
            let content = input["content"]
                .as_str()
                .ok_or("write_file: missing 'content' field")?;

            let full_path = validate_path(&ctx.workspace_root, path)?;
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create dirs: {}", e))?;
            }
            std::fs::write(&full_path, content)
                .map_err(|e| format!("failed to write {}: {}", path, e))?;

            Ok((format!("Successfully wrote {} bytes to {}", content.len(), path), false))
        }

        "list_files" => {
            let path = input["path"]
                .as_str()
                .ok_or("list_files: missing 'path' field")?;

            let full_path = validate_path(&ctx.workspace_root, path)?;
            if !full_path.is_dir() {
                return Err(format!("{} is not a directory", path));
            }

            let entries = std::fs::read_dir(&full_path)
                .map_err(|e| format!("failed to list {}: {}", path, e))?;

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

        "finish" => {
            let summary = input["summary"]
                .as_str()
                .unwrap_or("Agent finished without summary");
            Ok((summary.to_string(), true))
        }

        other => Err(format!("unknown tool: {}", other)),
    }
}
