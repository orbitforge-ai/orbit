use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, Semaphore};

use crate::db::DbPool;
use crate::events::emitter::emit_run_state_changed;
use crate::executor::process;
use crate::executor::state_machine::{transition, ExecutorEvent};
use crate::models::run::RunState;
use crate::models::task::{ShellCommandConfig, Task};

const DEFAULT_AGENT_ID: &str = "01HZDEFAULTDEFAULTDEFAULTDA";

/// Request sent to the executor engine to start a run.
#[derive(Debug, Clone)]
pub struct RunRequest {
    pub run_id: String,
    pub task: Task,
    pub schedule_id: Option<String>,
    pub trigger: String,
}

/// Newtype wrapping the sender half — stored as Tauri managed state.
#[derive(Clone)]
pub struct ExecutorTx(pub mpsc::UnboundedSender<RunRequest>);

/// The background execution engine.
/// Receives RunRequests and spawns tokio tasks per run.
pub struct ExecutorEngine {
    db: DbPool,
    rx: mpsc::UnboundedReceiver<RunRequest>,
    app: tauri::AppHandle,
    /// Global semaphore limiting total concurrent runs for the default agent.
    semaphore: Arc<Semaphore>,
    /// Directory where log files are written.
    log_dir: PathBuf,
}

impl ExecutorEngine {
    pub fn new(
        db: DbPool,
        rx: mpsc::UnboundedReceiver<RunRequest>,
        app: tauri::AppHandle,
        log_dir: PathBuf,
    ) -> Self {
        Self {
            db,
            rx,
            app,
            semaphore: Arc::new(Semaphore::new(10)), // default: 10 concurrent
            log_dir,
        }
    }

    pub async fn run(mut self) {
        tracing::info!("ExecutorEngine started");
        while let Some(req) = self.rx.recv().await {
            let db = self.db.clone();
            let app = self.app.clone();
            let semaphore = self.semaphore.clone();
            let log_dir = self.log_dir.clone();

            tokio::spawn(async move {
                let permit = semaphore.acquire_owned().await.expect("semaphore closed");

                if let Err(e) = run_one(req, db, app, log_dir).await {
                    tracing::error!("run failed: {}", e);
                }

                drop(permit);
            });
        }
        tracing::warn!("ExecutorEngine channel closed — shutting down");
    }
}

async fn run_one(
    req: RunRequest,
    db: DbPool,
    app: tauri::AppHandle,
    log_dir: PathBuf,
) -> Result<(), String> {
    let run_id = req.run_id.clone();
    let task = req.task;

    // Transition: pending → running (state update in DB)
    update_run_state(&db, &run_id, &RunState::Running, None, None, None)?;
    emit_run_state_changed(&app, &run_id, RunState::Pending.as_str(), RunState::Running.as_str());

    let log_path = log_dir.join(format!("{}.log", run_id));
    let timeout_secs = task.max_duration_seconds as u64;

    let result = match task.kind.as_str() {
        "shell_command" => {
            let cfg: ShellCommandConfig =
                serde_json::from_value(task.config.clone()).map_err(|e| e.to_string())?;
            process::run_shell(&run_id, &cfg, &log_path, timeout_secs, &app).await
        }
        other => Err(format!("unsupported task kind: {}", other)),
    };

    match result {
        Ok(proc_result) => {
            let is_success = proc_result.exit_code == 0;
            let event = if is_success {
                ExecutorEvent::Succeeded {
                    exit_code: proc_result.exit_code,
                    duration_ms: proc_result.duration_ms,
                }
            } else {
                ExecutorEvent::Failed {
                    exit_code: Some(proc_result.exit_code),
                    reason: format!("exit code {}", proc_result.exit_code),
                }
            };

            let next_state = transition(&RunState::Running, &event)
                .unwrap_or(RunState::Failure);

            update_run_state(
                &db,
                &run_id,
                &next_state,
                Some(proc_result.exit_code),
                Some(proc_result.duration_ms),
                None,
            )?;

            emit_run_state_changed(
                &app,
                &run_id,
                RunState::Running.as_str(),
                next_state.as_str(),
            );
        }
        Err(reason) => {
            let next_state = if reason == "timed out" {
                RunState::TimedOut
            } else {
                RunState::Failure
            };

            let metadata = serde_json::json!({ "error": reason });
            update_run_state(&db, &run_id, &next_state, Some(-1), None, Some(metadata))?;
            emit_run_state_changed(&app, &run_id, RunState::Running.as_str(), next_state.as_str());
        }
    }

    // Update agent heartbeat
    let _ = update_agent_heartbeat(&db, DEFAULT_AGENT_ID);

    Ok(())
}

fn update_run_state(
    db: &DbPool,
    run_id: &str,
    state: &RunState,
    exit_code: Option<i32>,
    duration_ms: Option<i64>,
    metadata: Option<serde_json::Value>,
) -> Result<(), String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();

    let finished_at = match state {
        RunState::Success | RunState::Failure | RunState::TimedOut | RunState::Cancelled => {
            Some(now.clone())
        }
        _ => None,
    };

    let started_at = match state {
        RunState::Running => Some(now.clone()),
        _ => None,
    };

    conn.execute(
        "UPDATE runs SET
            state = ?1,
            exit_code = COALESCE(?2, exit_code),
            duration_ms = COALESCE(?3, duration_ms),
            started_at = COALESCE(?4, started_at),
            finished_at = COALESCE(?5, finished_at),
            metadata = COALESCE(?6, metadata)
         WHERE id = ?7",
        rusqlite::params![
            state.as_str(),
            exit_code,
            duration_ms,
            started_at,
            finished_at,
            metadata.map(|m| m.to_string()),
            run_id,
        ],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

fn update_agent_heartbeat(db: &DbPool, agent_id: &str) -> Result<(), String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE agents SET heartbeat_at = ?1, updated_at = ?1 WHERE id = ?2",
        rusqlite::params![now, agent_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
