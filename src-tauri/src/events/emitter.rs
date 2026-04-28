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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_summary: Option<String>,
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
pub struct UserQuestionPayload {
    pub request_id: String,
    pub run_id: String,
    pub session_id: Option<String>,
    pub question: String,
    pub choices: Option<Vec<String>>,
    pub allow_custom: bool,
    pub multi_select: bool,
    pub context: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_agent_id: Option<String>,
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
    crate::shim::ws::broadcast("run:log_chunk", &payload);
}

pub fn emit_run_state_changed(
    app: &tauri::AppHandle,
    run_id: &str,
    previous_state: &str,
    new_state: &str,
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
    crate::shim::ws::broadcast("run:state_changed", &payload);
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
    crate::shim::ws::broadcast("agent:llm_chunk", &payload);
}

pub fn emit_agent_iteration(
    app: &tauri::AppHandle,
    run_id: &str,
    iteration: u32,
    action: &str,
    tool_name: Option<&str>,
    total_tokens: u32,
    finish_summary: Option<&str>,
) {
    let payload = AgentIterationPayload {
        run_id: run_id.to_string(),
        iteration,
        action: action.to_string(),
        tool_name: tool_name.map(|s| s.to_string()),
        total_tokens,
        finish_summary: finish_summary.map(|s| s.to_string()),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    if let Err(e) = app.emit("agent:iteration", &payload) {
        warn!("failed to emit agent:iteration: {}", e);
    }
    crate::shim::ws::broadcast("agent:iteration", &payload);
}

pub fn emit_agent_content_block(
    app: &tauri::AppHandle,
    run_id: &str,
    iteration: u32,
    block_type: &str,
    block: serde_json::Value,
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
    crate::shim::ws::broadcast("agent:content_block", &payload);
}

pub fn emit_agent_tool_result(
    app: &tauri::AppHandle,
    run_id: &str,
    iteration: u32,
    tool_use_id: &str,
    content: &str,
    is_error: bool,
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
    crate::shim::ws::broadcast("agent:tool_result", &payload);
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
    crate::shim::ws::broadcast("chat:context_update", &payload);
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
    crate::shim::ws::broadcast("agent:sub_agents_spawned", &payload);
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
    crate::shim::ws::broadcast("bus:message_sent", &event_payload);
}

pub fn emit_user_question(
    app: &tauri::AppHandle,
    request_id: &str,
    run_id: &str,
    session_id: Option<&str>,
    question: &str,
    choices: Option<&[String]>,
    allow_custom: bool,
    multi_select: bool,
    context: Option<&str>,
) {
    let payload = UserQuestionPayload {
        request_id: request_id.to_string(),
        run_id: run_id.to_string(),
        session_id: session_id.map(|value| value.to_string()),
        question: question.to_string(),
        choices: choices.map(|value| value.to_vec()),
        allow_custom,
        multi_select,
        context: context.map(|value| value.to_string()),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    if let Err(e) = app.emit("user:question", &payload) {
        warn!("failed to emit user:question: {}", e);
    }
    crate::shim::ws::broadcast("user:question", &payload);
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
    pub risk_level: String, // "moderate" | "dangerous"
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
    crate::shim::ws::broadcast("permission:request", &payload);
}

pub fn emit_permission_cancelled(app: &tauri::AppHandle, request_id: &str, run_id: &str) {
    let payload = PermissionCancelledPayload {
        request_id: request_id.to_string(),
        run_id: run_id.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    if let Err(e) = app.emit("permission:cancelled", &payload) {
        warn!("failed to emit permission:cancelled: {}", e);
    }
    crate::shim::ws::broadcast("permission:cancelled", &payload);
}

pub fn emit_agent_created(
    app: &tauri::AppHandle,
    agent: crate::models::agent::Agent,
    role_id: Option<String>,
) {
    let payload = AgentCreatedPayload { agent, role_id };
    if let Err(e) = app.emit("agent:created", &payload) {
        warn!("failed to emit agent:created: {}", e);
    }
    crate::shim::ws::broadcast("agent:created", &payload);
}

pub fn emit_agent_updated(
    app: &tauri::AppHandle,
    agent: crate::models::agent::Agent,
    previous_agent_id: Option<String>,
) {
    let payload = AgentUpdatedPayload {
        agent,
        previous_agent_id,
    };
    if let Err(e) = app.emit("agent:updated", &payload) {
        warn!("failed to emit agent:updated: {}", e);
    }
    crate::shim::ws::broadcast("agent:updated", &payload);
}

pub fn emit_agent_deleted(app: &tauri::AppHandle, agent_id: &str) {
    let payload = AgentDeletedPayload {
        agent_id: agent_id.to_string(),
    };
    if let Err(e) = app.emit("agent:deleted", &payload) {
        warn!("failed to emit agent:deleted: {}", e);
    }
    crate::shim::ws::broadcast("agent:deleted", &payload);
}

// ─── Compaction status events ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionStatusPayload {
    pub session_id: String,
    pub status: String, // "started" | "completed" | "failed" | "skipped"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub timestamp: String,
}

pub fn emit_compaction_status(app: &tauri::AppHandle, session_id: &str, status: &str) {
    emit_compaction_status_with_reason(app, session_id, status, None);
}

pub fn emit_compaction_status_with_reason(
    app: &tauri::AppHandle,
    session_id: &str,
    status: &str,
    reason: Option<&str>,
) {
    let payload = CompactionStatusPayload {
        session_id: session_id.to_string(),
        status: status.to_string(),
        reason: reason.map(|s| s.to_string()),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    if let Err(e) = app.emit("compaction:status", &payload) {
        warn!("failed to emit compaction:status: {}", e);
    }
    crate::shim::ws::broadcast("compaction:status", &payload);
}

pub fn emit_agent_config_changed(app: &tauri::AppHandle, agent_id: &str, role_id: Option<String>) {
    let payload = AgentConfigChangedPayload {
        agent_id: agent_id.to_string(),
        role_id,
    };
    if let Err(e) = app.emit("agent:config_changed", &payload) {
        warn!("failed to emit agent:config_changed: {}", e);
    }
    crate::shim::ws::broadcast("agent:config_changed", &payload);
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageReactionPayload {
    pub session_id: String,
    pub message_id: String,
    pub reaction_id: String,
    pub emoji: String,
    pub timestamp: String,
}

// ─── Terminal (PTY) events ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalChunkPayload {
    pub terminal_id: String,
    /// Base64-encoded raw bytes from the PTY master fd. Frontend decodes
    /// before handing to xterm.js so control sequences survive transport.
    pub data: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalExitPayload {
    pub terminal_id: String,
    pub code: i32,
    pub timestamp: String,
}

pub fn emit_terminal_chunk(app: &tauri::AppHandle, terminal_id: &str, data: &str) {
    let payload = TerminalChunkPayload {
        terminal_id: terminal_id.to_string(),
        data: data.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    if let Err(e) = app.emit("terminal:output_chunk", &payload) {
        warn!("failed to emit terminal:output_chunk: {}", e);
    }
    crate::shim::ws::broadcast("terminal:output_chunk", &payload);
}

pub fn emit_terminal_exit(app: &tauri::AppHandle, terminal_id: &str, code: i32) {
    let payload = TerminalExitPayload {
        terminal_id: terminal_id.to_string(),
        code,
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    if let Err(e) = app.emit("terminal:exit", &payload) {
        warn!("failed to emit terminal:exit: {}", e);
    }
    crate::shim::ws::broadcast("terminal:exit", &payload);
}

pub fn emit_message_reaction(
    app: &tauri::AppHandle,
    session_id: &str,
    message_id: &str,
    reaction_id: &str,
    emoji: &str,
    timestamp: &str,
) {
    let payload = MessageReactionPayload {
        session_id: session_id.to_string(),
        message_id: message_id.to_string(),
        reaction_id: reaction_id.to_string(),
        emoji: emoji.to_string(),
        timestamp: timestamp.to_string(),
    };
    if let Err(e) = app.emit("message:reaction", &payload) {
        warn!("failed to emit message:reaction: {}", e);
    }
    crate::shim::ws::broadcast("message:reaction", &payload);
}
