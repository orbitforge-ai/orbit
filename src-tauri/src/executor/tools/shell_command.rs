use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde_json::json;
use tauri::Manager;
use tracing::info;

use crate::events::emitter::emit_log_chunk;
use crate::executor::bg_processes::BgProcessRegistry;
use crate::executor::llm_provider::ToolDefinition;
use crate::executor::workspace;

use super::{context::ToolExecutionContext, ToolHandler};

/// Default timeout for shell_command tool calls in seconds.
const DEFAULT_SHELL_COMMAND_TIMEOUT_SECS: u64 = 120;
/// Maximum timeout for shell_command tool calls in seconds.
const MAX_SHELL_COMMAND_TIMEOUT_SECS: u64 = 600;
/// Maximum size of shell command output stored in tool results.
const MAX_SHELL_COMMAND_RESULT_LEN: usize = 50_000;

pub struct ShellCommandTool;

fn format_shell_command_result(stdout: &str, stderr: &str, exit_code: Option<i32>) -> String {
    let mut result = String::new();

    if !stdout.is_empty() {
        result.push_str(stdout);
    }
    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str("[stderr]\n");
        result.push_str(stderr);
    }
    if let Some(code) = exit_code {
        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(&format!("[exit code: {}]", code));
    }

    if result.len() > MAX_SHELL_COMMAND_RESULT_LEN {
        result.truncate(MAX_SHELL_COMMAND_RESULT_LEN);
        result.push_str("\n[output truncated]");
    }

    result
}

async fn read_shell_output(path: &Path) -> String {
    tokio::fs::read_to_string(path).await.unwrap_or_default()
}

async fn cleanup_shell_output(paths: &[PathBuf]) {
    for path in paths {
        let _ = tokio::fs::remove_file(path).await;
    }
}

async fn execute_shell_command(
    workspace_root: &Path,
    command: &str,
    timeout_secs: u64,
    app: &tauri::AppHandle,
    run_id: &str,
) -> Result<String, String> {
    let temp_dir = std::env::temp_dir();
    let temp_id = ulid::Ulid::new().to_string();
    let stdout_path = temp_dir.join(format!("orbit-shell-{}-stdout.log", temp_id));
    let stderr_path = temp_dir.join(format!("orbit-shell-{}-stderr.log", temp_id));

    let stdout_file = std::fs::File::create(&stdout_path)
        .map_err(|e| format!("failed to create stdout capture: {}", e))?;
    let stderr_file = std::fs::File::create(&stderr_path)
        .map_err(|e| format!("failed to create stderr capture: {}", e))?;

    let mut child = tokio::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .current_dir(workspace_root)
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("failed to execute command: {}", e))?;

    let status =
        match tokio::time::timeout(tokio::time::Duration::from_secs(timeout_secs), child.wait())
            .await
        {
            Ok(Ok(status)) => Some(status),
            Ok(Err(e)) => {
                cleanup_shell_output(&[stdout_path.clone(), stderr_path.clone()]).await;
                return Err(format!("failed to execute command: {}", e));
            }
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                None
            }
        };

    let stdout = read_shell_output(&stdout_path).await;
    let stderr = read_shell_output(&stderr_path).await;
    cleanup_shell_output(&[stdout_path, stderr_path]).await;

    if !stdout.is_empty() {
        let lines: Vec<(String, String)> = stdout
            .lines()
            .map(|line| ("stdout".to_string(), line.to_string()))
            .collect();
        emit_log_chunk(app, run_id, lines);
    }
    if !stderr.is_empty() {
        let lines: Vec<(String, String)> = stderr
            .lines()
            .map(|line| ("stderr".to_string(), line.to_string()))
            .collect();
        emit_log_chunk(app, run_id, lines);
    }

    match status {
        Some(status) => Ok(format_shell_command_result(
            &stdout,
            &stderr,
            Some(status.code().unwrap_or(-1)),
        )),
        None => {
            let partial = format_shell_command_result(&stdout, &stderr, None);
            if partial.is_empty() {
                Err(format!("shell_command timed out after {}s", timeout_secs))
            } else {
                Err(format!(
                    "shell_command timed out after {}s\n\nPartial output:\n{}",
                    timeout_secs, partial
                ))
            }
        }
    }
}

#[async_trait::async_trait]
impl ToolHandler for ShellCommandTool {
    fn name(&self) -> &'static str {
        "shell_command"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Execute a shell command in the agent's workspace directory. Returns stdout and stderr. Supports background execution and process management for long-running commands.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "description": format!(
                            "Maximum time to wait before aborting the command (default: {}, max: {}).",
                            DEFAULT_SHELL_COMMAND_TIMEOUT_SECS,
                            MAX_SHELL_COMMAND_TIMEOUT_SECS
                        )
                    },
                    "run_in_background": {
                        "type": "boolean",
                        "description": "If true, run the command in the background and return immediately with a process_id."
                    },
                    "process_action": {
                        "type": "string",
                        "enum": ["list", "poll", "kill"],
                        "description": "Manage background processes. Use without command."
                    },
                    "process_id": {
                        "type": "string",
                        "description": "Background process ID for poll or kill actions."
                    }
                },
                "oneOf": [
                    { "required": ["command"] },
                    { "required": ["process_action"] }
                ]
            }),
        }
    }

    async fn execute(
        &self,
        ctx: &ToolExecutionContext,
        input: &serde_json::Value,
        app: &tauri::AppHandle,
        run_id: &str,
    ) -> Result<(String, bool), String> {
        let registry = app.state::<BgProcessRegistry>();
        if let Some(action) = input["process_action"].as_str() {
            return handle_process_action(ctx, &registry, input, action).await;
        }

        let command = input["command"]
            .as_str()
            .ok_or("shell_command: missing 'command' field")?;
        let timeout_secs = input["timeout_seconds"]
            .as_u64()
            .unwrap_or(DEFAULT_SHELL_COMMAND_TIMEOUT_SECS)
            .clamp(1, MAX_SHELL_COMMAND_TIMEOUT_SECS);
        let run_in_background = input["run_in_background"].as_bool().unwrap_or(false);

        info!(
            run_id = run_id,
            command = command,
            timeout_secs = timeout_secs,
            run_in_background = run_in_background,
            "agent tool: shell_command"
        );

        let workspace_root = ctx.workspace_root();
        std::fs::create_dir_all(&workspace_root)
            .map_err(|e| format!("failed to create workspace: {}", e))?;

        if run_in_background {
            let bg_root = workspace::agent_dir(&ctx.agent_id).join("bg");
            let summary = registry
                .spawn(&ctx.agent_id, command, &workspace_root, &bg_root)
                .await?;
            let result = serde_json::to_string_pretty(&summary)
                .map_err(|e| format!("failed to serialize background process result: {}", e))?;
            return Ok((result, false));
        }

        let result =
            execute_shell_command(&workspace_root, command, timeout_secs, app, run_id).await?;

        Ok((result, false))
    }
}

async fn handle_process_action(
    ctx: &ToolExecutionContext,
    registry: &BgProcessRegistry,
    input: &serde_json::Value,
    action: &str,
) -> Result<(String, bool), String> {
    match action {
        "list" => {
            let processes = registry.list(&ctx.agent_id).await;
            let result = serde_json::to_string_pretty(&processes)
                .map_err(|e| format!("failed to serialize process list: {}", e))?;
            Ok((result, false))
        }
        "poll" => {
            let process_id = input["process_id"]
                .as_str()
                .ok_or("shell_command: missing 'process_id' for poll action")?;
            let process = registry.poll(&ctx.agent_id, process_id).await?;
            let result = serde_json::to_string_pretty(&process)
                .map_err(|e| format!("failed to serialize process poll result: {}", e))?;
            Ok((result, false))
        }
        "kill" => {
            let process_id = input["process_id"]
                .as_str()
                .ok_or("shell_command: missing 'process_id' for kill action")?;
            let process = registry.kill(&ctx.agent_id, process_id).await?;
            let result = serde_json::to_string_pretty(&process)
                .map_err(|e| format!("failed to serialize process kill result: {}", e))?;
            Ok((result, false))
        }
        other => Err(format!("shell_command: unknown process_action '{}'", other)),
    }
}
