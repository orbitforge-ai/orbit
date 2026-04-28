//! Build `PtySpawnSpec`s for each CLI Orbit can launch in the embedded terminal.
//!
//! The supported kinds are documented in `CliKind`. For Claude / Codex /
//! Gemini we inject the chat session's agent system prompt so the CLI is
//! Orbit-aware out of the gate. The "Shell" kind is a raw escape hatch that
//! launches the user's $SHELL with no injection.
//!
//! NOTE: full MCP wiring (so `claude` in the terminal sees Orbit tools as
//! `mcp__orbit__*`) is intentionally not done in this v1 — building a real
//! `ToolExecutionContext` outside `executor::session_agent` would be a larger
//! refactor. The hook points are marked with `// TODO(mcp-terminal)` below
//! and the launcher already accepts an `mcp_config_path` so a future change
//! can wire it without touching the PTY layer.
use std::path::PathBuf;

use serde::Deserialize;
use tracing::warn;

use crate::executor::cli_common;
use crate::executor::pty_session::PtySpawnSpec;

/// Which CLI to launch. `Shell` is the generic escape hatch.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum CliKind {
    Claude,
    Codex,
    Gemini,
    Shell,
}

/// Inputs the launcher needs from the caller (terminal Tauri command).
pub struct TerminalContext {
    /// Working directory the CLI should start in. Defaults to `$HOME` if None.
    pub cwd: Option<PathBuf>,
    /// Agent's `system_prompt.md` content, injected via the CLI's
    /// per-session system-prompt mechanism. Empty string skips injection.
    pub system_prompt: String,
    /// Initial PTY size — clients usually reissue `resize_terminal` after
    /// xterm.js fits, so this is mostly for the very first redraw.
    pub rows: u16,
    pub cols: u16,
    /// Optional path to a Claude-style MCP config JSON. Wire-ready but not
    /// yet populated by `commands::terminals` — see TODO above.
    pub mcp_config_path: Option<PathBuf>,
}

pub fn build_pty_spec(kind: &CliKind, ctx: TerminalContext) -> Result<PtySpawnSpec, String> {
    let mut env = base_env(&ctx);

    let (program, args) = match kind {
        CliKind::Claude => {
            let bin = cli_common::resolve_cli("claude").ok_or_else(|| {
                "claude CLI not found on PATH. Install it from https://docs.claude.com/en/docs/claude-code.".to_string()
            })?;
            let mut args: Vec<String> = Vec::new();
            if !ctx.system_prompt.is_empty() {
                args.push("--append-system-prompt".to_string());
                args.push(ctx.system_prompt.clone());
            }
            if let Some(path) = &ctx.mcp_config_path {
                args.push("--mcp-config".to_string());
                args.push(path.display().to_string());
                args.push("--allowedTools".to_string());
                args.push("mcp__orbit".to_string());
            }
            (bin, args)
        }
        CliKind::Codex => {
            let bin = cli_common::resolve_cli("codex").ok_or_else(|| {
                "codex CLI not found on PATH. Install it from https://github.com/openai/codex.".to_string()
            })?;
            // Codex doesn't take a system prompt flag; surface it via a
            // CODEX-recognized env var so newer versions can pick it up, and
            // also CWD-injecting AGENTS.md is the user's escape hatch.
            if !ctx.system_prompt.is_empty() {
                env.push((
                    "CODEX_APPEND_SYSTEM_PROMPT".to_string(),
                    ctx.system_prompt.clone(),
                ));
            }
            (bin, Vec::new())
        }
        CliKind::Gemini => {
            let bin = cli_common::resolve_cli("gemini").ok_or_else(|| {
                "gemini CLI not found on PATH. Install it from https://github.com/google-gemini/gemini-cli.".to_string()
            })?;
            if !ctx.system_prompt.is_empty() {
                env.push((
                    "GEMINI_SYSTEM_INSTRUCTION".to_string(),
                    ctx.system_prompt.clone(),
                ));
            }
            (bin, Vec::new())
        }
        CliKind::Shell => {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| {
                if cfg!(target_os = "macos") {
                    "/bin/zsh".to_string()
                } else {
                    "/bin/bash".to_string()
                }
            });
            let bin = PathBuf::from(&shell);
            if !bin.exists() {
                warn!("shell binary missing at {}", bin.display());
            }
            (bin, vec!["-l".to_string()])
        }
    };

    Ok(PtySpawnSpec {
        program,
        args,
        cwd: ctx.cwd,
        env,
        rows: ctx.rows,
        cols: ctx.cols,
    })
}

/// Base environment shared by every spawned CLI. Inherits PATH from the
/// parent (so Tauri-on-macOS doesn't lose Homebrew bins) and pins TERM /
/// COLORTERM so xterm.js renders the same as a real terminal.
fn base_env(_ctx: &TerminalContext) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = Vec::new();

    // Pass through PATH, HOME, USER, LANG so the CLI behaves the same as in a
    // login shell. Other vars are intentionally NOT inherited — keep the env
    // surface small to avoid leaking secrets into spawned children.
    for key in ["PATH", "HOME", "USER", "LANG", "LC_ALL", "SHELL"] {
        if let Ok(value) = std::env::var(key) {
            env.push((key.to_string(), value));
        }
    }

    env.push(("TERM".to_string(), "xterm-256color".to_string()));
    env.push(("COLORTERM".to_string(), "truecolor".to_string()));

    env
}
