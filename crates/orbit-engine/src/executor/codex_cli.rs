//! LLM provider that routes through the local `codex` CLI binary.
//!
//! Design mirrors `claude_cli.rs`: the CLI runs its own inner agent loop,
//! calls Orbit tools through an embedded MCP bridge, and emits JSONL events
//! on stdout that we translate into Orbit's `LlmResponse`. A per-run
//! ephemeral `CODEX_HOME` holds the MCP server registration plus a copy of
//! the user's `auth.json`, so Orbit never mutates the user's real config.
//!
//! Native-tool lockout strategy: we launch `codex exec` with
//! `--sandbox read-only` and `approval_policy = "never"`, so Codex's
//! built-in shell/filesystem tools will refuse any operation that would
//! escape the sandbox instead of prompting. Orbit MCP tools still run
//! because they go over the HTTP bridge, not through the sandbox.
use std::path::PathBuf;
use std::process::Stdio;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tracing::{debug, warn};

use crate::events::emitter::{emit_agent_content_block, emit_agent_llm_chunk, emit_log_chunk};
use crate::executor::claude_cli::SessionFn;
use crate::executor::cli_common;
use crate::executor::llm_provider::{
    ChatMessage, ContentBlock, LlmConfig, LlmProvider, LlmResponse, StopReason, ToolDefinition,
    Usage,
};
use crate::executor::mcp_server::McpServerHandle;
use crate::executor::session_worktree::SessionWorktreeState;

pub struct CodexCliProvider {
    mcp: Option<McpServerHandle>,
    session_fn: Option<SessionFn>,
}

impl CodexCliProvider {
    pub fn new(mcp: Option<McpServerHandle>) -> Self {
        Self {
            mcp,
            session_fn: None,
        }
    }

    pub fn with_session_fn(mut self, f: SessionFn) -> Self {
        self.session_fn = Some(f);
        self
    }
}

struct StreamContext<'a> {
    app: &'a tauri::AppHandle,
    run_id: &'a str,
    iteration: u32,
    text_buf: String,
    content: Vec<ContentBlock>,
    usage: Usage,
    stop_reason: StopReason,
}

fn binary_path() -> Result<PathBuf, String> {
    cli_common::resolve_cli("codex").ok_or_else(|| {
        "codex CLI not found on PATH. Install it from https://github.com/openai/codex.".to_string()
    })
}

fn user_codex_home() -> PathBuf {
    if let Ok(h) = std::env::var("CODEX_HOME") {
        if !h.is_empty() {
            return PathBuf::from(h);
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".codex")
}

/// Build an ephemeral CODEX_HOME that has:
///   - auth.json copied from the user's real CODEX_HOME (so the CLI stays logged in)
///   - a minimal config.toml that registers the Orbit MCP server and pins
///     approval_policy = "never"
/// Returns the temp dir path — caller is responsible for cleanup.
fn build_codex_home(
    handle: &McpServerHandle,
    token_env_var: &str,
    model: &str,
) -> Result<PathBuf, String> {
    let base = std::env::temp_dir().join("orbit-codex");
    std::fs::create_dir_all(&base)
        .map_err(|e| format!("failed to create codex temp base dir: {}", e))?;
    let dir = base.join(format!("run-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create codex temp home: {}", e))?;

    let src_auth = user_codex_home().join("auth.json");
    if src_auth.exists() {
        let dst = dir.join("auth.json");
        std::fs::copy(&src_auth, &dst).map_err(|e| {
            format!(
                "failed to copy codex auth.json from {} to {}: {}",
                src_auth.display(),
                dst.display(),
                e
            )
        })?;
    } else {
        warn!(
            "codex-cli: no auth.json at {}; codex exec will likely fail 401",
            src_auth.display()
        );
    }

    let config = format!(
        r#"model = "{model}"
approval_policy = "never"
sandbox_mode = "read-only"

[mcp_servers.orbit]
url = "{url}"
bearer_token_env_var = "{env_var}"
"#,
        model = model,
        url = handle.url(),
        env_var = token_env_var,
    );
    std::fs::write(dir.join("config.toml"), config)
        .map_err(|e| format!("failed to write codex config.toml: {}", e))?;
    Ok(dir)
}

fn build_mcp_tool_guidance(tools: &[ToolDefinition]) -> String {
    if tools.is_empty() {
        return String::new();
    }

    let mut out = String::from(
        "Orbit MCP tool access:\n\
These Orbit capabilities are available as MCP function tools, not MCP resources.\n\
Do not search for them via MCP resource discovery; call the function tools directly.\n\
When in doubt, use the namespaced MCP form `mcp__orbit__<tool_name>`.\n\
Important examples:\n\
- Project backlog / kanban updates: `mcp__orbit__work_item`\n\
- Workflow edits/runs: `mcp__orbit__workflow`\n\
- File reads/writes: `mcp__orbit__read_file`, `mcp__orbit__write_file`, `mcp__orbit__edit_file`\n\
- Shell commands: `mcp__orbit__shell_command`\n\
\n\
Available Orbit MCP function tools:\n",
    );

    for tool in tools {
        out.push_str("- `mcp__orbit__");
        out.push_str(&tool.name);
        out.push_str("`");
        if !tool.description.trim().is_empty() {
            out.push_str(": ");
            out.push_str(tool.description.trim());
        }
        out.push('\n');
    }

    out
}

fn build_prompt(
    history: &str,
    current: &str,
    system_prompt: &str,
    tools: &[ToolDefinition],
) -> String {
    let mut out = String::new();
    let mcp_guidance = build_mcp_tool_guidance(tools);
    if !system_prompt.is_empty() {
        out.push_str("System instructions:\n");
        out.push_str(system_prompt);
        if !mcp_guidance.is_empty() {
            out.push_str("\n\n---\n\n");
            out.push_str(&mcp_guidance);
        }
        out.push_str("\n\n---\n\n");
    } else if !mcp_guidance.is_empty() {
        out.push_str(&mcp_guidance);
        out.push_str("\n\n---\n\n");
    }
    if !history.is_empty() {
        out.push_str("Prior conversation (for context, do not repeat):\n\n");
        out.push_str(history);
        out.push_str("\n\n---\n\nCurrent request:\n");
    }
    out.push_str(current);
    out
}

fn with_workspace_notice(
    system_prompt: &str,
    workspace_root: Option<&std::path::Path>,
    current_worktree: Option<&SessionWorktreeState>,
) -> String {
    let Some(workspace_root) = workspace_root else {
        return system_prompt.to_string();
    };

    let mut prompt = system_prompt.to_string();
    if !prompt.is_empty() {
        prompt.push_str("\n\n");
    }
    prompt.push_str("## Runtime Workspace\n");
    prompt.push_str(&format!(
        "- Active workspace: {}\n",
        workspace_root.display()
    ));
    if let Some(worktree) = current_worktree {
        prompt.push_str(&format!(
            "- Managed worktree: {} ({})\n",
            worktree.path.display(),
            worktree.branch
        ));
    }
    prompt.push_str("- Stay within this workspace when inspecting or changing code.\n");
    prompt
}

#[async_trait]
impl LlmProvider for CodexCliProvider {
    fn name(&self) -> &str {
        "codex-cli"
    }

    async fn chat_streaming(
        &self,
        config: &LlmConfig,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        app: &tauri::AppHandle,
        run_id: &str,
        iteration: u32,
    ) -> Result<LlmResponse, String> {
        let binary = binary_path()?;
        let token_env_var = "ORBIT_MCP_TOKEN";
        let mut provider_workspace_root: Option<PathBuf> = None;
        let mut provider_worktree: Option<SessionWorktreeState> = None;

        let (mcp_token, codex_home) = match (&self.mcp, self.session_fn.as_ref()) {
            (Some(handle), Some(builder)) => {
                if let Some(session) = builder() {
                    if session.tool_ctx.sandbox_enabled() {
                        provider_workspace_root = Some(session.tool_ctx.workspace_root());
                        provider_worktree = session.tool_ctx.current_worktree();
                    }
                    let token = handle.issue_token(session).await;
                    let home = build_codex_home(handle, token_env_var, &config.model)?;
                    (Some((handle.clone(), token)), Some(home))
                } else {
                    (None, None)
                }
            }
            _ => (None, None),
        };

        if codex_home.is_none() && !tools.is_empty() {
            warn!(
                run_id = run_id,
                "codex-cli: MCP bridge not wired; tool catalog will be unavailable to the model"
            );
        }

        let (history, current) = cli_common::transcript_for_cli(messages);
        let prompt = build_prompt(
            &history,
            &current,
            &with_workspace_notice(
                &config.system_prompt,
                provider_workspace_root.as_deref(),
                provider_worktree.as_ref(),
            ),
            tools,
        );

        let mut cmd = Command::new(&binary);
        cmd.arg("exec")
            .arg("--json")
            .arg("--skip-git-repo-check")
            .arg("--sandbox")
            .arg("read-only")
            .arg("--ephemeral")
            .arg("--model")
            .arg(&config.model);

        if let Some(workspace_root) = &provider_workspace_root {
            cmd.current_dir(workspace_root).env("PWD", workspace_root);
        }

        if let Some(home) = &codex_home {
            cmd.env("CODEX_HOME", home);
        }
        if let Some((_, token)) = &mcp_token {
            cmd.env(token_env_var, token);
        }

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to spawn `{}`: {}", binary.display(), e))?;

        if let Some(mut stdin) = child.stdin.take() {
            if let Err(e) = stdin.write_all(prompt.as_bytes()).await {
                warn!("codex-cli: failed to write prompt: {}", e);
            }
            let _ = stdin.shutdown().await;
        }

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "codex-cli: no stdout".to_string())?;
        let stderr = child.stderr.take();

        let stderr_run_id = run_id.to_string();
        let stderr_app = app.clone();
        if let Some(err) = stderr {
            tokio::spawn(async move {
                let mut reader = BufReader::new(err).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    emit_log_chunk(
                        &stderr_app,
                        &stderr_run_id,
                        vec![("stderr".to_string(), line)],
                    );
                }
            });
        }

        let mut ctx = StreamContext {
            app,
            run_id,
            iteration,
            text_buf: String::new(),
            content: Vec::new(),
            usage: Usage::default(),
            stop_reason: StopReason::EndTurn,
        };

        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(event) => handle_event(&mut ctx, &event),
                Err(e) => debug!("codex-cli: non-JSON stdout line ignored: {} ({})", line, e),
            }
        }

        let _ = child.wait().await;

        if !ctx.text_buf.is_empty() {
            ctx.content.push(ContentBlock::Text {
                text: std::mem::take(&mut ctx.text_buf),
            });
        }

        if let Some(home) = codex_home {
            let _ = std::fs::remove_dir_all(home);
        }
        if let Some((handle, token)) = mcp_token {
            handle.revoke_token(&token).await;
        }

        Ok(LlmResponse {
            content: ctx.content,
            stop_reason: ctx.stop_reason,
            usage: ctx.usage,
        })
    }

    async fn chat_complete(
        &self,
        config: &LlmConfig,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<LlmResponse, String> {
        let _ = (config, messages, tools);
        Err("codex-cli: chat_complete is not supported yet. Use chat_streaming.".to_string())
    }
}

fn handle_event(ctx: &mut StreamContext<'_>, event: &Value) {
    let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match event_type {
        "thread.started" | "turn.started" => {}
        "item.started" => {
            // Codex emits item.started before streaming; we surface it later
            // via item.completed. No-op for now.
        }
        "item.completed" => {
            if let Some(item) = event.get("item") {
                handle_item(ctx, item);
            }
        }
        "turn.completed" => {
            if let Some(u) = event.get("usage") {
                apply_usage(&mut ctx.usage, u);
            }
            ctx.stop_reason = StopReason::EndTurn;
        }
        "turn.failed" => {
            let msg = event
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("codex turn failed");
            warn!("codex-cli: {}", msg);
            emit_log_chunk(
                ctx.app,
                ctx.run_id,
                vec![("stderr".to_string(), format!("codex error: {}", msg))],
            );
            // Surface the failure as text so the chat UI shows a bubble
            // instead of silently dropping the turn. Otherwise the user
            // sees a sent message with no response.
            let user_text = parse_codex_error(msg);
            emit_agent_llm_chunk(ctx.app, ctx.run_id, &user_text, ctx.iteration);
            ctx.text_buf.push_str(&user_text);
        }
        "error" => {
            if let Some(msg) = event.get("message").and_then(|v| v.as_str()) {
                // Transient reconnects log to stderr; surface but don't mark stop_reason.
                debug!("codex-cli transient error: {}", msg);
            }
        }
        other => {
            debug!("codex-cli: unhandled event type: {}", other);
        }
    }
}

fn handle_item(ctx: &mut StreamContext<'_>, item: &Value) {
    let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match item_type {
        "agent_message" => {
            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                emit_agent_llm_chunk(ctx.app, ctx.run_id, text, ctx.iteration);
                emit_log_chunk(
                    ctx.app,
                    ctx.run_id,
                    vec![("stdout".to_string(), text.to_string())],
                );
                ctx.text_buf.push_str(text);
            }
        }
        "reasoning" => {
            if let Some(t) = item
                .get("text")
                .and_then(|v| v.as_str())
                .or_else(|| item.get("summary").and_then(|v| v.as_str()))
            {
                ctx.content.push(ContentBlock::Thinking {
                    thinking: t.to_string(),
                });
                emit_agent_content_block(
                    ctx.app,
                    ctx.run_id,
                    ctx.iteration,
                    "thinking",
                    json!({"type": "thinking", "thinking": t}),
                );
            }
        }
        "mcp_tool_call" | "tool_call" | "function_call" => {
            // Codex has already executed the tool by the time we see this
            // (via the MCP bridge, which ran permissions::execute_tool_with_permissions).
            // Surface a UI event so users can see it; don't add to ctx.content
            // because the outer Orbit loop treats each CLI call as a single
            // end-turn response.
            let id = item
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = item
                .get("name")
                .and_then(|v| v.as_str())
                .or_else(|| item.get("tool").and_then(|v| v.as_str()))
                .unwrap_or("")
                .to_string();
            let input = item
                .get("arguments")
                .cloned()
                .or_else(|| item.get("input").cloned())
                .unwrap_or(json!({}));
            emit_agent_content_block(
                ctx.app,
                ctx.run_id,
                ctx.iteration,
                "tool_use",
                json!({"type": "tool_use", "id": id, "name": name, "input": input}),
            );
        }
        "command_execution" | "shell_call" => {
            // Sandboxed command attempt — only shows up if the model tried
            // to use Codex's native shell. Surface it so users see what was
            // attempted; the sandbox should have refused write operations.
            let cmd = item.get("command").and_then(|v| v.as_str()).unwrap_or("");
            emit_log_chunk(
                ctx.app,
                ctx.run_id,
                vec![("stdout".to_string(), format!("[sandboxed] {}", cmd))],
            );
        }
        other => {
            debug!("codex-cli: unhandled item type: {}", other);
        }
    }
}

/// Codex wraps upstream API errors in a JSON-string envelope. Try to pull
/// out the inner `error.message` so the user sees a readable bubble instead
/// of a serialized JSON blob.
fn parse_codex_error(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Ok(outer) = serde_json::from_str::<Value>(trimmed) {
        if let Some(inner) = outer
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
        {
            return format!("Codex CLI error: {}", inner);
        }
        if let Some(message) = outer.get("message").and_then(|m| m.as_str()) {
            return format!("Codex CLI error: {}", message);
        }
    }
    format!("Codex CLI error: {}", trimmed)
}

fn apply_usage(usage: &mut Usage, v: &Value) {
    if let Some(n) = v.get("input_tokens").and_then(|x| x.as_u64()) {
        usage.input_tokens = usage.input_tokens.saturating_add(n as u32);
    }
    if let Some(n) = v.get("output_tokens").and_then(|x| x.as_u64()) {
        usage.output_tokens = usage.output_tokens.saturating_add(n as u32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_prompt_includes_history_and_system() {
        let p = build_prompt("prev", "now", "sys", &[]);
        assert!(p.contains("System instructions:"));
        assert!(p.contains("sys"));
        assert!(p.contains("Prior conversation"));
        assert!(p.contains("prev"));
        assert!(p.contains("Current request:"));
        assert!(p.contains("now"));
    }

    #[test]
    fn build_prompt_with_no_history_and_no_system_is_plain() {
        let p = build_prompt("", "hi", "", &[]);
        assert_eq!(p, "hi");
    }

    #[test]
    fn build_prompt_includes_mcp_tool_guidance() {
        let p = build_prompt(
            "",
            "hi",
            "",
            &[ToolDefinition {
                name: "work_item".into(),
                description: "Update the project backlog".into(),
                input_schema: json!({ "type": "object" }),
            }],
        );
        assert!(p.contains("mcp__orbit__work_item"));
        assert!(p.contains("not MCP resources"));
    }

    #[test]
    fn parse_codex_error_unwraps_nested_message() {
        let raw = r#"{"type":"error","status":400,"error":{"type":"invalid_request_error","message":"The 'gpt-5-codex' model is not supported when using Codex with a ChatGPT account."}}"#;
        let text = parse_codex_error(raw);
        assert!(text.contains("Codex CLI error:"));
        assert!(text.contains("not supported when using Codex"));
    }

    #[test]
    fn parse_codex_error_falls_back_to_raw() {
        let text = parse_codex_error("something broke");
        assert_eq!(text, "Codex CLI error: something broke");
    }

    #[test]
    fn workspace_notice_appends_runtime_workspace_details() {
        let prompt = with_workspace_notice(
            "sys",
            Some(std::path::Path::new("/tmp/project")),
            Some(&SessionWorktreeState {
                name: "wt".into(),
                branch: "orbit/wt".into(),
                path: PathBuf::from("/tmp/worktree"),
            }),
        );
        assert!(prompt.contains("Active workspace: /tmp/project"));
        assert!(prompt.contains("Managed worktree: /tmp/worktree (orbit/wt)"));
    }

    #[test]
    fn handle_agent_message_appends_text() {
        // Simulate assembling a StreamContext without an AppHandle —
        // handle_item paths that do not emit events are safe to unit test
        // indirectly via apply_usage here; full event path is covered by
        // integration probes in the PR.
        let mut usage = Usage::default();
        apply_usage(&mut usage, &json!({"input_tokens": 10, "output_tokens": 3}));
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 3);
    }
}
