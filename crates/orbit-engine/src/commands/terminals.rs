//! Tauri commands for the embedded terminal (TUI).
//!
//! Lifecycle: `open_terminal` → 0..N `write_terminal`/`resize_terminal` →
//! `close_terminal`. The PTY also dies if the child process exits on its own
//! (the wait thread emits `terminal:exit` and the registry entry is reaped on
//! the next `close_terminal` call).
use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::app_context::AppContext;
use crate::executor::cli_launcher::{self, CliKind, TerminalContext};
use crate::executor::pty_session::{PtyRegistry, PtySession};
use crate::executor::workspace as exec_workspace;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenTerminalArgs {
    /// Optional chat session id. When provided, the terminal launches in that
    /// agent's workspace dir with the agent's `system_prompt.md` injected. When
    /// absent, falls back to `$HOME` and no system prompt is injected.
    pub session_id: Option<String>,
    pub kind: CliKind,
    pub rows: u16,
    pub cols: u16,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenTerminalResponse {
    pub terminal_id: String,
}

#[tauri::command]
pub async fn open_terminal(
    args: OpenTerminalArgs,
    app: State<'_, AppContext>,
    registry: State<'_, PtyRegistry>,
    tauri_app: tauri::AppHandle,
) -> Result<OpenTerminalResponse, String> {
    let (cwd, system_prompt) = match &args.session_id {
        Some(sid) => {
            let agent_id = lookup_agent_id(sid, &app).await?;
            let cwd = exec_workspace::agent_workspace_dir(&agent_id);
            if !cwd.exists() {
                if let Err(e) = std::fs::create_dir_all(&cwd) {
                    return Err(format!("failed to ensure workspace dir: {}", e));
                }
            }
            // Missing system_prompt.md is fine — just skip injection.
            let prompt = exec_workspace::read_workspace_file(&agent_id, "system_prompt.md")
                .unwrap_or_default();
            (cwd, prompt)
        }
        None => {
            let home = std::env::var("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::path::PathBuf::from("/"));
            (home, String::new())
        }
    };

    let ctx = TerminalContext {
        cwd: Some(cwd),
        system_prompt,
        rows: args.rows.max(1),
        cols: args.cols.max(1),
        mcp_config_path: None,
    };

    let spec = cli_launcher::build_pty_spec(&args.kind, ctx)?;
    let terminal_id = format!("term-{}", ulid::Ulid::new());
    let session = PtySession::spawn(terminal_id.clone(), spec, tauri_app, Vec::new())?;

    registry.insert(terminal_id.clone(), session).await;

    Ok(OpenTerminalResponse { terminal_id })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteTerminalArgs {
    pub terminal_id: String,
    /// Base64-encoded byte payload. xterm.js gives us strings, the frontend
    /// encodes before sending so we can faithfully forward control sequences.
    pub data: String,
}

#[tauri::command]
pub async fn write_terminal(
    args: WriteTerminalArgs,
    registry: State<'_, PtyRegistry>,
) -> Result<(), String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&args.data)
        .map_err(|e| format!("invalid base64 data: {}", e))?;
    registry.write(&args.terminal_id, &bytes).await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResizeTerminalArgs {
    pub terminal_id: String,
    pub rows: u16,
    pub cols: u16,
}

#[tauri::command]
pub async fn resize_terminal(
    args: ResizeTerminalArgs,
    registry: State<'_, PtyRegistry>,
) -> Result<(), String> {
    registry
        .resize(&args.terminal_id, args.rows.max(1), args.cols.max(1))
        .await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseTerminalArgs {
    pub terminal_id: String,
}

#[tauri::command]
pub async fn close_terminal(
    args: CloseTerminalArgs,
    registry: State<'_, PtyRegistry>,
) -> Result<(), String> {
    registry.close(&args.terminal_id).await;
    Ok(())
}

async fn lookup_agent_id(session_id: &str, app: &AppContext) -> Result<String, String> {
    app.repos
        .chat()
        .session_meta(session_id)
        .await
        .map(|meta| meta.agent_id)
        .map_err(|e| format!("chat session not found: {}", e))
}

// ─── HTTP shim registration ─────────────────────────────────────────────────

pub fn register_http(reg: &mut crate::shim::registry::Registry) {
    reg.register("open_terminal", |ctx, args| async move {
        let app = ctx.app()?;
        let a: OpenTerminalArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
        let resp = open_terminal(
            a,
            app.state::<AppContext>(),
            app.state::<PtyRegistry>(),
            app.clone(),
        )
        .await?;
        serde_json::to_value(resp).map_err(|e| e.to_string())
    });
    reg.register("write_terminal", |ctx, args| async move {
        let app = ctx.app()?;
        let a: WriteTerminalArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
        write_terminal(a, app.state::<PtyRegistry>()).await?;
        Ok(serde_json::Value::Null)
    });
    reg.register("resize_terminal", |ctx, args| async move {
        let app = ctx.app()?;
        let a: ResizeTerminalArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
        resize_terminal(a, app.state::<PtyRegistry>()).await?;
        Ok(serde_json::Value::Null)
    });
    reg.register("close_terminal", |ctx, args| async move {
        let app = ctx.app()?;
        let a: CloseTerminalArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
        close_terminal(a, app.state::<PtyRegistry>()).await?;
        Ok(serde_json::Value::Null)
    });
}
