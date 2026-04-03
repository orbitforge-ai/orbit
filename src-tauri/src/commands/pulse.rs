use serde::{Deserialize, Serialize};
use tracing::info;
use ulid::Ulid;

use crate::db::DbPool;
use crate::executor::workspace;
use crate::models::schedule::RecurringConfig;
use crate::scheduler::converter::{next_n_runs, to_cron};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseConfig {
    pub enabled: bool,
    pub content: String,
    pub schedule: Option<RecurringConfig>,
    pub task_id: Option<String>,
    pub schedule_id: Option<String>,
    pub session_id: Option<String>,
    pub next_run_at: Option<String>,
    pub last_run_at: Option<String>,
}

/// Get the pulse configuration for an agent.
#[tauri::command]
pub async fn get_pulse_config(
    agent_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<PulseConfig, String> {
    let pool = db.0.clone();
    let aid = agent_id.clone();

    // Read pulse.md content
    let content = workspace::read_workspace_file(&agent_id, "pulse.md")
        .unwrap_or_default();

    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;

        // Find pulse task: task with agent_id and tags containing "pulse"
        let pulse_task: Option<(String, bool)> = conn
            .query_row(
                "SELECT id, enabled FROM tasks WHERE agent_id = ?1 AND tags LIKE '%\"pulse\"%'",
                rusqlite::params![aid],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)),
            )
            .ok();

        let (task_id, schedule_id, enabled, schedule, next_run_at, last_run_at) =
            if let Some((tid, _task_enabled)) = pulse_task {
                // Find schedule for this task
                let sched: Option<(String, String, bool, Option<String>, Option<String>)> = conn
                    .query_row(
                        "SELECT id, config, enabled, next_run_at, last_run_at FROM schedules WHERE task_id = ?1",
                        rusqlite::params![tid],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, bool>(2)?,
                                row.get::<_, Option<String>>(3)?,
                                row.get::<_, Option<String>>(4)?,
                            ))
                        },
                    )
                    .ok();

                if let Some((sid, config_str, sched_enabled, next, last)) = sched {
                    let cfg: Option<RecurringConfig> =
                        serde_json::from_str(&config_str).ok();
                    (Some(tid), Some(sid), sched_enabled, cfg, next, last)
                } else {
                    (Some(tid), None, false, None, None, None)
                }
            } else {
                (None, None, false, None, None, None)
            };

        // Find pulse chat session
        let session_id: Option<String> = conn
            .query_row(
                "SELECT id FROM chat_sessions WHERE agent_id = ?1 AND session_type = 'pulse'",
                rusqlite::params![aid],
                |row| row.get(0),
            )
            .ok();

        Ok(PulseConfig {
            enabled,
            content,
            schedule,
            task_id,
            schedule_id,
            session_id,
            next_run_at,
            last_run_at,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Update pulse configuration: content, schedule, and enabled state.
#[tauri::command]
pub async fn update_pulse(
    agent_id: String,
    content: String,
    schedule_config: RecurringConfig,
    enabled: bool,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, crate::db::cloud::CloudClientState>,
) -> Result<PulseConfig, String> {
    let pool = db.0.clone();
    let aid = agent_id.clone();

    // Write pulse.md content to workspace
    workspace::write_workspace_file(&agent_id, "pulse.md", &content)?;

    // Sync model_config (includes pulse.md) to cloud
    let _ = crate::commands::workspace::sync_model_config_to_cloud(
        &agent_id, db.0.clone(), cloud.inner().clone(),
    ).await;

    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();

        // ── Find or create pulse task ────────────────────────────────────
        let existing_task_id: Option<String> = conn
            .query_row(
                "SELECT id FROM tasks WHERE agent_id = ?1 AND tags LIKE '%\"pulse\"%'",
                rusqlite::params![aid],
                |row| row.get(0),
            )
            .ok();

        let task_config = serde_json::json!({ "goal": content });
        let task_config_str = task_config.to_string();

        // Look up agent name for human-readable task naming
        let agent_name: String = conn
            .query_row(
                "SELECT name FROM agents WHERE id = ?1",
                rusqlite::params![aid],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| aid.chars().take(20).collect());

        let task_id = if let Some(tid) = existing_task_id {
            // Update existing task config and name
            conn.execute(
                "UPDATE tasks SET config = ?1, name = ?2, updated_at = ?3 WHERE id = ?4",
                rusqlite::params![task_config_str, format!("[Pulse] {}", agent_name), now, tid],
            )
            .map_err(|e| e.to_string())?;
            tid
        } else {
            // Create new pulse task
            let tid = Ulid::new().to_string();
            conn.execute(
                "INSERT INTO tasks (id, name, description, kind, config, max_duration_seconds, max_retries, retry_delay_seconds, concurrency_policy, tags, agent_id, enabled, created_at, updated_at)
                 VALUES (?1, ?2, 'Automated pulse schedule', 'agent_loop', ?3, 7200, 0, 60, 'skip', '[\"pulse\"]', ?4, 1, ?5, ?5)",
                rusqlite::params![
                    tid,
                    format!("[Pulse] {}", agent_name),
                    task_config_str,
                    aid,
                    now,
                ],
            )
            .map_err(|e| e.to_string())?;
            tid
        };

        // ── Find or create schedule ──────────────────────────────────────
        let existing_schedule_id: Option<String> = conn
            .query_row(
                "SELECT id FROM schedules WHERE task_id = ?1",
                rusqlite::params![task_id],
                |row| row.get(0),
            )
            .ok();

        let sched_config_str =
            serde_json::to_string(&schedule_config).map_err(|e| e.to_string())?;

        // Compute next_run_at
        let next_run_at = if enabled {
            to_cron(&schedule_config)
                .ok()
                .map(|_| next_n_runs(&schedule_config, 1).into_iter().next())
                .flatten()
        } else {
            None
        };

        let schedule_id = if let Some(sid) = existing_schedule_id {
            // Update existing schedule
            conn.execute(
                "UPDATE schedules SET config = ?1, enabled = ?2, next_run_at = ?3, updated_at = ?4 WHERE id = ?5",
                rusqlite::params![sched_config_str, enabled as i64, next_run_at, now, sid],
            )
            .map_err(|e| e.to_string())?;
            sid
        } else {
            // Create new schedule
            let sid = Ulid::new().to_string();
            conn.execute(
                "INSERT INTO schedules (id, task_id, kind, config, enabled, next_run_at, created_at, updated_at)
                 VALUES (?1, ?2, 'recurring', ?3, ?4, ?5, ?6, ?6)",
                rusqlite::params![sid, task_id, sched_config_str, enabled as i64, next_run_at, now],
            )
            .map_err(|e| e.to_string())?;
            sid
        };

        // ── Ensure Pulse chat session exists ─────────────────────────────
        let session_id: String = conn
            .query_row(
                "SELECT id FROM chat_sessions WHERE agent_id = ?1 AND session_type = 'pulse'",
                rusqlite::params![aid],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| {
                let sid = Ulid::new().to_string();
                let _ = conn.execute(
                    "INSERT INTO chat_sessions (
                       id, agent_id, title, archived, session_type, parent_session_id, source_bus_message_id,
                       chain_depth, execution_state, finish_summary, terminal_error, created_at, updated_at
                     ) VALUES (?1, ?2, 'Pulse', 0, 'pulse', NULL, NULL, 0, NULL, NULL, NULL, ?3, ?3)",
                    rusqlite::params![sid, aid, now],
                );
                sid
            });

        // Get last_run_at from schedule
        let last_run_at: Option<String> = conn
            .query_row(
                "SELECT last_run_at FROM schedules WHERE id = ?1",
                rusqlite::params![schedule_id],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        info!(agent_id = %aid, enabled = enabled, "Pulse configuration updated");

        Ok(PulseConfig {
            enabled,
            content,
            schedule: Some(schedule_config),
            task_id: Some(task_id),
            schedule_id: Some(schedule_id),
            session_id: Some(session_id),
            next_run_at,
            last_run_at,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
