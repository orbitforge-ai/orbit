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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerTickPayload {
    pub next_runs: Vec<NextRunEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NextRunEntry {
    pub schedule_id: String,
    pub task_id: String,
    pub run_at: String,
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
}
