use serde::Serialize;
use tauri::Emitter;
use tracing::warn;

// ─── Event Payloads ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
  pub stream: String, // "stdout" | "stderr"
  pub line: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunLogChunkPayload {
  pub run_id: String,
  pub lines: Vec<LogLine>,
  pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStateChangedPayload {
  pub run_id: String,
  pub previous_state: String,
  pub new_state: String,
  pub timestamp: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerTickPayload {
  pub next_runs: Vec<NextRunEntry>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NextRunEntry {
  pub schedule_id: String,
  pub task_id: String,
  pub run_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLlmChunkPayload {
  pub run_id: String,
  pub delta: String,
  pub iteration: u32,
  pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentIterationPayload {
  pub run_id: String,
  pub iteration: u32,
  pub action: String, // "llm_call" | "tool_exec" | "finished"
  pub tool_name: Option<String>,
  pub total_tokens: u32,
  pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentContentBlockPayload {
  pub run_id: String,
  pub iteration: u32,
  pub block_type: String, // "thinking" | "tool_use"
  pub block: serde_json::Value,
  pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolResultPayload {
  pub run_id: String,
  pub iteration: u32,
  pub tool_use_id: String,
  pub content: String,
  pub is_error: bool,
  pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatContextUpdatePayload {
  pub session_id: String,
  pub input_tokens: u32,
  pub output_tokens: u32,
  pub context_window_size: u32,
  pub usage_percent: f64,
  pub timestamp: String,
}

// ─── Emit helpers ────────────────────────────────────────────────────────────

pub fn emit_log_chunk(app: &tauri::AppHandle, run_id: &str, lines: Vec<(String, String)>) {
  let payload = RunLogChunkPayload {
    run_id: run_id.to_string(),
    lines: lines
      .into_iter()
      .map(|(stream, line)| LogLine { stream, line })
      .collect(),
    timestamp: chrono::Utc::now().to_rfc3339(),
  };
  if let Err(e) = app.emit("run:log_chunk", &payload) {
    warn!("failed to emit run:log_chunk: {}", e);
  }
}

pub fn emit_run_state_changed(
  app: &tauri::AppHandle,
  run_id: &str,
  previous_state: &str,
  new_state: &str
) {
  let payload = RunStateChangedPayload {
    run_id: run_id.to_string(),
    previous_state: previous_state.to_string(),
    new_state: new_state.to_string(),
    timestamp: chrono::Utc::now().to_rfc3339(),
  };
  if let Err(e) = app.emit("run:state_changed", &payload) {
    warn!("failed to emit run:state_changed: {}", e);
  }
}

pub fn emit_agent_llm_chunk(app: &tauri::AppHandle, run_id: &str, delta: &str, iteration: u32) {
  let payload = AgentLlmChunkPayload {
    run_id: run_id.to_string(),
    delta: delta.to_string(),
    iteration,
    timestamp: chrono::Utc::now().to_rfc3339(),
  };
  if let Err(e) = app.emit("agent:llm_chunk", &payload) {
    warn!("failed to emit agent:llm_chunk: {}", e);
  }
}

pub fn emit_agent_iteration(
  app: &tauri::AppHandle,
  run_id: &str,
  iteration: u32,
  action: &str,
  tool_name: Option<&str>,
  total_tokens: u32
) {
  let payload = AgentIterationPayload {
    run_id: run_id.to_string(),
    iteration,
    action: action.to_string(),
    tool_name: tool_name.map(|s| s.to_string()),
    total_tokens,
    timestamp: chrono::Utc::now().to_rfc3339(),
  };
  if let Err(e) = app.emit("agent:iteration", &payload) {
    warn!("failed to emit agent:iteration: {}", e);
  }
}

pub fn emit_agent_content_block(
  app: &tauri::AppHandle,
  run_id: &str,
  iteration: u32,
  block_type: &str,
  block: serde_json::Value
) {
  let payload = AgentContentBlockPayload {
    run_id: run_id.to_string(),
    iteration,
    block_type: block_type.to_string(),
    block,
    timestamp: chrono::Utc::now().to_rfc3339(),
  };
  if let Err(e) = app.emit("agent:content_block", &payload) {
    warn!("failed to emit agent:content_block: {}", e);
  }
}

pub fn emit_agent_tool_result(
  app: &tauri::AppHandle,
  run_id: &str,
  iteration: u32,
  tool_use_id: &str,
  content: &str,
  is_error: bool
) {
  let payload = AgentToolResultPayload {
    run_id: run_id.to_string(),
    iteration,
    tool_use_id: tool_use_id.to_string(),
    content: content.to_string(),
    is_error,
    timestamp: chrono::Utc::now().to_rfc3339(),
  };
  if let Err(e) = app.emit("agent:tool_result", &payload) {
    warn!("failed to emit agent:tool_result: {}", e);
  }
}

pub fn emit_chat_context_update(
  app: &tauri::AppHandle,
  session_id: &str,
  input_tokens: u32,
  output_tokens: u32,
  context_window_size: u32,
) {
  let usage_percent = if context_window_size > 0 {
    (input_tokens as f64 / context_window_size as f64) * 100.0
  } else {
    0.0
  };
  let payload = ChatContextUpdatePayload {
    session_id: session_id.to_string(),
    input_tokens,
    output_tokens,
    context_window_size,
    usage_percent,
    timestamp: chrono::Utc::now().to_rfc3339(),
  };
  if let Err(e) = app.emit("chat:context_update", &payload) {
    warn!("failed to emit chat:context_update: {}", e);
  }
}
