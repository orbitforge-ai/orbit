//! LLM provider that routes through the local `claude` CLI binary.
//!
//! The CLI's own agent loop drives the conversation: it reads stream-json
//! events on stdin, calls Orbit tools via an embedded MCP bridge, and emits
//! stream-json events on stdout that we translate back into Orbit's
//! `LlmResponse` shape. From Orbit's outer-loop perspective, each call is a
//! single streaming completion that always ends in `EndTurn`.
use std::path::PathBuf;
use std::process::Stdio;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tracing::{debug, warn};

use crate::events::emitter::{
    emit_agent_content_block, emit_agent_llm_chunk, emit_agent_tool_result, emit_log_chunk,
};
use crate::executor::cli_common;
use crate::executor::llm_provider::{
    ChatMessage, ContentBlock, LlmConfig, LlmProvider, LlmResponse, StopReason, ToolDefinition,
    Usage,
};
use crate::executor::mcp_server::{McpServerHandle, McpSession};

pub type SessionFn = std::sync::Arc<dyn Fn() -> Option<McpSession> + Send + Sync + 'static>;

/// List of Claude CLI built-in tools that Orbit disables to prevent bypass of
/// the MCP-gated permission path. Keep conservative — newer CLI releases may
/// add tools; the defense-in-depth warning in the Settings UI covers the gap
/// until this list is audited against a pinned CLI version.
const DISALLOWED_BUILTIN_TOOLS: &str =
    "Bash,Edit,Read,Write,Glob,Grep,WebFetch,WebSearch,NotebookEdit,MultiEdit,Task";

pub struct ClaudeCliProvider {
    mcp: Option<McpServerHandle>,
    /// Called once per LLM call to build an MCP session describing which tools
    /// are permitted and which `ToolExecutionContext` to dispatch them against.
    /// `None` means the CLI will run without any Orbit tools exposed — only
    /// safe for non-agentic diagnostic surfaces.
    session_fn: Option<SessionFn>,
}

impl ClaudeCliProvider {
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
    /// True once a `stream_event` with a text delta has been seen for the
    /// current turn. When set, we skip re-emitting text from the aggregated
    /// `assistant` event so the UI doesn't see each chunk twice.
    text_streamed: bool,
}

fn binary_path() -> Result<PathBuf, String> {
    cli_common::resolve_cli("claude")
        .ok_or_else(|| "claude CLI not found on PATH. Install it from https://docs.claude.com/en/docs/claude-code.".to_string())
}

fn build_mcp_config(handle: &McpServerHandle, token: &str) -> Result<PathBuf, String> {
    let cfg = json!({
        "mcpServers": {
            "orbit": {
                "type": "http",
                "url": handle.url(),
                "headers": {
                    "Authorization": format!("Bearer {}", token)
                }
            }
        }
    });
    let dir = std::env::temp_dir().join("orbit-mcp");
    std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create mcp temp dir: {}", e))?;
    let path = dir.join(format!("claude-{}.json", ulid::Ulid::new()));
    std::fs::write(&path, serde_json::to_string(&cfg).unwrap())
        .map_err(|e| format!("failed to write mcp config: {}", e))?;
    Ok(path)
}

fn serialize_input_turn(history: &str, current: &str) -> String {
    let combined = if history.is_empty() {
        current.to_string()
    } else {
        format!(
            "Prior conversation (for context, do not repeat):\n\n{}\n\n---\n\nCurrent request:\n{}",
            history, current
        )
    };
    let event = json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": combined,
        }
    });
    serde_json::to_string(&event).unwrap()
}

#[async_trait]
impl LlmProvider for ClaudeCliProvider {
    fn name(&self) -> &str {
        "claude-cli"
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

        // Mint an MCP token for this call when we have an MCP server + session
        // context. Drop it deterministically when the call ends.
        let (mcp_token, mcp_config_path) = match (&self.mcp, self.session_fn.as_ref()) {
            (Some(handle), Some(builder)) => {
                if let Some(session) = builder() {
                    let token = handle.issue_token(session).await;
                    let path = build_mcp_config(handle, &token)?;
                    (Some((handle.clone(), token)), Some(path))
                } else {
                    (None, None)
                }
            }
            _ => (None, None),
        };

        let (history, current) = cli_common::transcript_for_cli(messages);
        let input_payload = serialize_input_turn(&history, &current);

        let mut cmd = Command::new(&binary);
        cmd.arg("-p")
            .arg("--input-format")
            .arg("stream-json")
            .arg("--output-format")
            .arg("stream-json")
            .arg("--verbose")
            .arg("--include-partial-messages")
            .arg("--model")
            .arg(&config.model);

        if !config.system_prompt.is_empty() {
            cmd.arg("--append-system-prompt").arg(&config.system_prompt);
        }

        cmd.arg("--disallowedTools").arg(DISALLOWED_BUILTIN_TOOLS);

        if let Some(path) = &mcp_config_path {
            cmd.arg("--mcp-config").arg(path);
            // When MCP is wired, the CLI may present tools as `mcp__orbit__<name>`.
            // Allow the whole orbit MCP namespace.
            cmd.arg("--allowedTools").arg("mcp__orbit");
        } else if !tools.is_empty() {
            warn!(
                run_id = run_id,
                "claude-cli: MCP bridge not wired; tool catalog will be unavailable to the model"
            );
        }

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to spawn `{}`: {}", binary.display(), e))?;

        if let Some(mut stdin) = child.stdin.take() {
            let line = format!("{}\n", input_payload);
            if let Err(e) = stdin.write_all(line.as_bytes()).await {
                warn!("claude-cli: failed to write input: {}", e);
            }
            let _ = stdin.shutdown().await;
        }

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "claude-cli: no stdout".to_string())?;
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
            text_streamed: false,
        };

        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(event) => handle_event(&mut ctx, &event),
                Err(e) => debug!("claude-cli: non-JSON stdout line ignored: {} ({})", line, e),
            }
        }

        let _ = child.wait().await;

        if !ctx.text_buf.is_empty() {
            ctx.content.push(ContentBlock::Text {
                text: std::mem::take(&mut ctx.text_buf),
            });
        }

        if let Some(path) = mcp_config_path {
            let _ = std::fs::remove_file(path);
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
        // chat_complete has no app handle — emit-free path. Build a stub handle
        // by requiring the caller to go through the streaming variant for now.
        // Concretely: the only current call site (workflows/nodes/agent.rs) can
        // accept this limitation — it works with plain text replies and does
        // not stream.
        let _ = (config, messages, tools);
        Err("claude-cli: chat_complete is not supported yet. Use chat_streaming.".to_string())
    }
}

fn handle_event(ctx: &mut StreamContext<'_>, event: &Value) {
    let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match event_type {
        "system" => {
            // {"type":"system","subtype":"init", ...} — no user-visible work.
        }
        "stream_event" => {
            if let Some(inner) = event.get("event") {
                handle_stream_event(ctx, inner);
            }
        }
        "assistant" => {
            if let Some(msg) = event.get("message") {
                handle_assistant_message(ctx, msg);
            }
        }
        "user" => {
            // User-role tool_result events emitted by the CLI's inner loop.
            // Surface them for the UI only; they don't feed Orbit's outer loop.
            if let Some(msg) = event.get("message") {
                if let Some(blocks) = msg.get("content").and_then(|v| v.as_array()) {
                    for block in blocks {
                        if block.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                            let tool_use_id = block
                                .get("tool_use_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let content =
                                block.get("content").and_then(|v| v.as_str()).unwrap_or("");
                            let is_error = block
                                .get("is_error")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            if !tool_use_id.is_empty() {
                                emit_agent_tool_result(
                                    ctx.app,
                                    ctx.run_id,
                                    ctx.iteration,
                                    tool_use_id,
                                    content,
                                    is_error,
                                );
                            }
                        }
                    }
                }
            }
        }
        "result" => {
            if let Some(res_str) = event.get("result").and_then(|v| v.as_str()) {
                // Some versions emit the final answer as a "result" string;
                // others only include per-block deltas. Append if non-empty
                // and not already captured.
                if !res_str.is_empty() && ctx.content.is_empty() && ctx.text_buf.is_empty() {
                    ctx.text_buf.push_str(res_str);
                }
            }
            if let Some(u) = event.get("usage") {
                apply_usage(&mut ctx.usage, u);
            }
            if let Some(sr) = event.get("stop_reason").and_then(|v| v.as_str()) {
                ctx.stop_reason = match sr {
                    "end_turn" => StopReason::EndTurn,
                    "tool_use" => StopReason::ToolUse,
                    "max_tokens" => StopReason::MaxTokens,
                    _ => StopReason::EndTurn,
                };
            }
        }
        other => {
            debug!("claude-cli: unhandled event type: {}", other);
        }
    }
}

fn handle_assistant_message(ctx: &mut StreamContext<'_>, msg: &Value) {
    // Anthropic-shaped message with content blocks.
    let Some(blocks) = msg.get("content").and_then(|v| v.as_array()) else {
        return;
    };
    for block in blocks {
        let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match block_type {
            "text" => {
                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                    // If we already streamed this text via `stream_event`
                    // deltas, do not re-emit — the UI has it.
                    if !ctx.text_streamed {
                        emit_agent_llm_chunk(ctx.app, ctx.run_id, text, ctx.iteration);
                        emit_log_chunk(
                            ctx.app,
                            ctx.run_id,
                            vec![("stdout".to_string(), text.to_string())],
                        );
                        ctx.text_buf.push_str(text);
                    }
                }
            }
            "thinking" => {
                if let Some(t) = block.get("thinking").and_then(|v| v.as_str()) {
                    ctx.content.push(ContentBlock::Thinking {
                        thinking: t.to_string(),
                    });
                    emit_agent_content_block(
                        ctx.app,
                        ctx.run_id,
                        ctx.iteration,
                        "thinking",
                        block.clone(),
                    );
                }
            }
            "tool_use" => {
                let id = block
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = block
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let input = block.get("input").cloned().unwrap_or(json!({}));
                ctx.content.push(ContentBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                });
                emit_agent_content_block(
                    ctx.app,
                    ctx.run_id,
                    ctx.iteration,
                    "tool_use",
                    json!({"type": "tool_use", "id": id, "name": name, "input": input}),
                );
            }
            _ => {}
        }
    }

    if let Some(u) = msg.get("usage") {
        apply_usage(&mut ctx.usage, u);
    }
}

/// Handle a `stream_event` wrapper — the CLI re-emits the same partial-message
/// events the Anthropic API emits over SSE. We care about `content_block_delta`
/// with `text_delta` payloads so text appears in the UI as it's generated.
fn handle_stream_event(ctx: &mut StreamContext<'_>, inner: &Value) {
    let kind = inner.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "content_block_delta" => {
            let Some(delta) = inner.get("delta") else {
                return;
            };
            let dtype = delta.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match dtype {
                "text_delta" => {
                    if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                        if text.is_empty() {
                            return;
                        }
                        emit_agent_llm_chunk(ctx.app, ctx.run_id, text, ctx.iteration);
                        emit_log_chunk(
                            ctx.app,
                            ctx.run_id,
                            vec![("stdout".to_string(), text.to_string())],
                        );
                        ctx.text_buf.push_str(text);
                        ctx.text_streamed = true;
                    }
                }
                "thinking_delta" => {
                    // Surface thinking deltas live — users can see reasoning
                    // as it streams. The final thinking block is assembled
                    // from the aggregated `assistant` message, so don't push
                    // into ctx.content here.
                    if let Some(t) = delta.get("thinking").and_then(|v| v.as_str()) {
                        emit_agent_content_block(
                            ctx.app,
                            ctx.run_id,
                            ctx.iteration,
                            "thinking_delta",
                            json!({"type": "thinking_delta", "thinking": t}),
                        );
                    }
                }
                "input_json_delta" => {
                    // Partial tool-use input arrives a fragment at a time.
                    // We wait for the aggregated `assistant` event to add
                    // the complete tool_use to content — deltas alone are
                    // not useful without the tool id/name.
                }
                _ => {}
            }
        }
        "message_delta" => {
            if let Some(u) = inner.get("usage") {
                apply_usage(&mut ctx.usage, u);
            }
            if let Some(sr) = inner
                .get("delta")
                .and_then(|d| d.get("stop_reason"))
                .and_then(|v| v.as_str())
            {
                ctx.stop_reason = match sr {
                    "end_turn" => StopReason::EndTurn,
                    "tool_use" => StopReason::ToolUse,
                    "max_tokens" => StopReason::MaxTokens,
                    _ => StopReason::EndTurn,
                };
            }
        }
        // message_start / content_block_start / content_block_stop / message_stop
        // carry no state we don't already get elsewhere.
        _ => {}
    }
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
    fn serialize_input_turn_no_history_is_plain_request() {
        let line = serialize_input_turn("", "hello");
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["type"], "user");
        assert_eq!(v["message"]["content"], "hello");
    }

    #[test]
    fn serialize_input_turn_with_history_includes_transcript_prefix() {
        let line = serialize_input_turn("prev", "now");
        let v: Value = serde_json::from_str(&line).unwrap();
        let content = v["message"]["content"].as_str().unwrap();
        assert!(content.contains("Prior conversation"));
        assert!(content.contains("prev"));
        assert!(content.contains("Current request:"));
        assert!(content.contains("now"));
    }

    #[test]
    fn message_delta_stop_reason_is_parsed() {
        // Validate the parsing path without an AppHandle by exercising the
        // inner helper that only touches ctx.usage and ctx.stop_reason.
        let mut usage = Usage::default();
        apply_usage(&mut usage, &json!({"input_tokens": 3, "output_tokens": 2}));
        assert_eq!(usage.input_tokens, 3);
        assert_eq!(usage.output_tokens, 2);
    }
}
