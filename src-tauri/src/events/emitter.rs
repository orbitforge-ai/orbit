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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubAgentsSpawnedPayload {
  pub parent_session_id: Option<String>,
  pub parent_run_id: Option<String>,
  pub sub_agent_session_ids: Vec<String>,
  pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BusMessageSentPayload {
  pub message_id: String,
  pub from_agent_id: String,
  pub to_agent_id: String,
  pub kind: String,
  pub payload: serde_json::Value,
  pub triggered_session_id: Option<String>,
  pub triggered_run_id: Option<String>,
  pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCreatedPayload {
  pub agent: crate::models::agent::Agent,
  pub role_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUpdatedPayload {
  pub agent: crate::models::agent::Agent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDeletedPayload {
  pub agent_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigChangedPayload {
  pub agent_id: String,
  pub role_id: Option<String>,
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

pub fn emit_sub_agents_spawned(
  app: &tauri::AppHandle,
  parent_session_id: Option<&str>,
  parent_run_id: Option<&str>,
  sub_agent_session_ids: Vec<String>,
) {
  let payload = SubAgentsSpawnedPayload {
    parent_session_id: parent_session_id.map(|s| s.to_string()),
    parent_run_id: parent_run_id.map(|s| s.to_string()),
    sub_agent_session_ids,
    timestamp: chrono::Utc::now().to_rfc3339(),
  };
  if let Err(e) = app.emit("agent:sub_agents_spawned", &payload) {
    warn!("failed to emit agent:sub_agents_spawned: {}", e);
  }
}

pub fn emit_bus_message_sent(
  app: &tauri::AppHandle,
  message_id: &str,
  from_agent_id: &str,
  to_agent_id: &str,
  kind: &str,
  payload: serde_json::Value,
  triggered_session_id: Option<&str>,
  triggered_run_id: Option<&str>,
) {
  let event_payload = BusMessageSentPayload {
    message_id: message_id.to_string(),
    from_agent_id: from_agent_id.to_string(),
    to_agent_id: to_agent_id.to_string(),
    kind: kind.to_string(),
    payload,
    triggered_session_id: triggered_session_id.map(|s| s.to_string()),
    triggered_run_id: triggered_run_id.map(|s| s.to_string()),
    timestamp: chrono::Utc::now().to_rfc3339(),
  };
  if let Err(e) = app.emit("bus:message_sent", &event_payload) {
    warn!("failed to emit bus:message_sent: {}", e);
  }
}

// ─── Permission events ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequestPayload {
  pub request_id: String,
  pub run_id: String,
  pub session_id: Option<String>,
  pub agent_id: String,
  pub tool_name: String,
  pub tool_input: serde_json::Value,
  pub risk_level: String,        // "moderate" | "dangerous"
  pub risk_description: String,
  pub suggested_pattern: String,
  pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionCancelledPayload {
  pub request_id: String,
  pub run_id: String,
  pub timestamp: String,
}

#[allow(clippy::too_many_arguments)]
pub fn emit_permission_request(
  app: &tauri::AppHandle,
  request_id: &str,
  run_id: &str,
  session_id: Option<&str>,
  agent_id: &str,
  tool_name: &str,
  tool_input: &serde_json::Value,
  risk_level: &str,
  risk_description: &str,
  suggested_pattern: &str,
) {
  let payload = PermissionRequestPayload {
    request_id: request_id.to_string(),
    run_id: run_id.to_string(),
    session_id: session_id.map(|s| s.to_string()),
    agent_id: agent_id.to_string(),
    tool_name: tool_name.to_string(),
    tool_input: tool_input.clone(),
    risk_level: risk_level.to_string(),
    risk_description: risk_description.to_string(),
    suggested_pattern: suggested_pattern.to_string(),
    timestamp: chrono::Utc::now().to_rfc3339(),
  };
  if let Err(e) = app.emit("permission:request", &payload) {
    warn!("failed to emit permission:request: {}", e);
  }
}

pub fn emit_permission_cancelled(
  app: &tauri::AppHandle,
  request_id: &str,
  run_id: &str,
) {
  let payload = PermissionCancelledPayload {
    request_id: request_id.to_string(),
    run_id: run_id.to_string(),
    timestamp: chrono::Utc::now().to_rfc3339(),
  };
  if let Err(e) = app.emit("permission:cancelled", &payload) {
    warn!("failed to emit permission:cancelled: {}", e);
  }
}

pub fn emit_agent_created(app: &tauri::AppHandle, agent: crate::models::agent::Agent, role_id: Option<String>) {
  if let Err(e) = app.emit("agent:created", AgentCreatedPayload { agent, role_id }) {
    warn!("failed to emit agent:created: {}", e);
  }
}

pub fn emit_agent_updated(app: &tauri::AppHandle, agent: crate::models::agent::Agent) {
  if let Err(e) = app.emit("agent:updated", AgentUpdatedPayload { agent }) {
    warn!("failed to emit agent:updated: {}", e);
  }
}

pub fn emit_agent_deleted(app: &tauri::AppHandle, agent_id: &str) {
  if let Err(e) = app.emit("agent:deleted", AgentDeletedPayload { agent_id: agent_id.to_string() }) {
    warn!("failed to emit agent:deleted: {}", e);
  }
}

pub fn emit_agent_config_changed(app: &tauri::AppHandle, agent_id: &str, role_id: Option<String>) {
  if let Err(e) = app.emit("agent:config_changed", AgentConfigChangedPayload { agent_id: agent_id.to_string(), role_id }) {
    warn!("failed to emit agent:config_changed: {}", e);
  }
}
