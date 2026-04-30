use serde::{Deserialize, Serialize};
use tracing::info;
use ulid::Ulid;

use crate::app_context::AppContext;
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

/// Get the pulse configuration for a given (agent, project) pair.
#[tauri::command]
pub async fn get_pulse_config(
    agent_id: String,
    project_id: String,
    app: tauri::State<'_, AppContext>,
) -> Result<PulseConfig, String> {
    get_pulse_config_inner(agent_id, project_id, &app).await
}

async fn get_pulse_config_inner(
    agent_id: String,
    project_id: String,
    app: &AppContext,
) -> Result<PulseConfig, String> {
    let pool = app.db.0.clone();
    let aid = agent_id.clone();
    let pid = project_id.clone();

    let content =
        workspace::read_project_agent_file(&project_id, &agent_id, "pulse.md").unwrap_or_default();

    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;

        let pulse_task: Option<(String, bool)> = conn
            .query_row(
                "SELECT id, enabled FROM tasks
                 WHERE agent_id = ?1
                   AND project_id = ?2
                   AND tags LIKE '%\"pulse\"%'
                   AND tenant_id = COALESCE((SELECT tenant_id FROM projects WHERE id = ?2),
                                            (SELECT tenant_id FROM agents WHERE id = ?1),
                                            'local')",
                rusqlite::params![aid, pid],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)),
            )
            .ok();

        let (task_id, schedule_id, enabled, schedule, next_run_at, last_run_at) =
            if let Some((tid, _task_enabled)) = pulse_task {
                let sched: Option<(String, String, bool, Option<String>, Option<String>)> = conn
                    .query_row(
                        "SELECT id, config, enabled, next_run_at, last_run_at
                         FROM schedules
                        WHERE task_id = ?1
                          AND tenant_id = COALESCE((SELECT tenant_id FROM tasks WHERE id = ?1), 'local')",
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
                    let cfg: Option<RecurringConfig> = serde_json::from_str(&config_str).ok();
                    (Some(tid), Some(sid), sched_enabled, cfg, next, last)
                } else {
                    (Some(tid), None, false, None, None, None)
                }
            } else {
                (None, None, false, None, None, None)
            };

        let session_id: Option<String> = conn
            .query_row(
                "SELECT id FROM chat_sessions
                 WHERE agent_id = ?1
                   AND project_id = ?2
                   AND session_type = 'pulse'
                   AND tenant_id = COALESCE((SELECT tenant_id FROM projects WHERE id = ?2),
                                            (SELECT tenant_id FROM agents WHERE id = ?1),
                                            'local')",
                rusqlite::params![aid, pid],
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

/// Update pulse configuration for a (agent, project) pair: content, schedule, and enabled state.
#[tauri::command]
pub async fn update_pulse(
    agent_id: String,
    project_id: String,
    content: String,
    schedule_config: RecurringConfig,
    enabled: bool,
    app: tauri::State<'_, AppContext>,
) -> Result<PulseConfig, String> {
    update_pulse_inner(
        agent_id,
        project_id,
        content,
        schedule_config,
        enabled,
        &app,
    )
    .await
}

async fn update_pulse_inner(
    agent_id: String,
    project_id: String,
    content: String,
    schedule_config: RecurringConfig,
    enabled: bool,
    app: &AppContext,
) -> Result<PulseConfig, String> {
    let pool = app.db.0.clone();
    let aid = agent_id.clone();
    let pid = project_id.clone();

    workspace::write_project_agent_file(&project_id, &agent_id, "pulse.md", &content)?;

    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();

        // ── Find or create pulse task (scoped by agent + project) ──────────
        let existing_task_id: Option<String> = conn
            .query_row(
                "SELECT id FROM tasks
                 WHERE agent_id = ?1
                   AND project_id = ?2
                   AND tags LIKE '%\"pulse\"%'
                   AND tenant_id = COALESCE((SELECT tenant_id FROM projects WHERE id = ?2),
                                            (SELECT tenant_id FROM agents WHERE id = ?1),
                                            'local')",
                rusqlite::params![aid, pid],
                |row| row.get(0),
            )
            .ok();

        let task_config = serde_json::json!({ "goal": content });
        let task_config_str = task_config.to_string();

        let agent_name: String = conn
            .query_row(
                "SELECT name FROM agents
                  WHERE id = ?1
                    AND tenant_id = COALESCE((SELECT tenant_id FROM projects WHERE id = ?2), 'local')",
                rusqlite::params![aid, pid],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| aid.chars().take(20).collect());

        let project_name: String = conn
            .query_row(
                "SELECT name FROM projects
                  WHERE id = ?1
                    AND tenant_id = COALESCE((SELECT tenant_id FROM agents WHERE id = ?2), 'local')",
                rusqlite::params![pid, aid],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| pid.chars().take(20).collect());

        let task_name = format!("[Pulse] {} — {}", agent_name, project_name);

        let task_id = if let Some(tid) = existing_task_id {
            conn.execute(
                "UPDATE tasks
                    SET config = ?1, name = ?2, updated_at = ?3
                  WHERE id = ?4
                    AND tenant_id = COALESCE((SELECT tenant_id FROM projects WHERE id = ?5),
                                             (SELECT tenant_id FROM agents WHERE id = ?6),
                                             'local')",
                rusqlite::params![task_config_str, task_name, now, tid, pid, aid],
            )
            .map_err(|e| e.to_string())?;
            tid
        } else {
            let tid = Ulid::new().to_string();
            conn.execute(
                "INSERT INTO tasks (id, name, description, kind, config, max_duration_seconds, max_retries, retry_delay_seconds, concurrency_policy, tags, agent_id, project_id, enabled, created_at, updated_at, tenant_id)
                 VALUES (?1, ?2, 'Automated pulse schedule', 'agent_loop', ?3, 7200, 0, 60, 'skip', '[\"pulse\"]', ?4, ?5, 1, ?6, ?6, COALESCE((SELECT tenant_id FROM agents WHERE id = ?4), 'local'))",
                rusqlite::params![
                    tid,
                    task_name,
                    task_config_str,
                    aid,
                    pid,
                    now,
                ],
            )
            .map_err(|e| e.to_string())?;
            tid
        };

        // ── Find or create schedule ──────────────────────────────────────
        let existing_schedule_id: Option<String> = conn
            .query_row(
                "SELECT id FROM schedules
                  WHERE task_id = ?1
                    AND tenant_id = COALESCE((SELECT tenant_id FROM tasks WHERE id = ?1), 'local')",
                rusqlite::params![task_id],
                |row| row.get(0),
            )
            .ok();

        let sched_config_str =
            serde_json::to_string(&schedule_config).map_err(|e| e.to_string())?;

        let next_run_at = if enabled {
            to_cron(&schedule_config)
                .ok()
                .map(|_| next_n_runs(&schedule_config, 1).into_iter().next())
                .flatten()
        } else {
            None
        };

        let schedule_id = if let Some(sid) = existing_schedule_id {
            conn.execute(
                "UPDATE schedules
                    SET config = ?1, enabled = ?2, next_run_at = ?3, updated_at = ?4
                  WHERE id = ?5
                    AND tenant_id = COALESCE((SELECT tenant_id FROM tasks WHERE id = ?6), 'local')",
                rusqlite::params![sched_config_str, enabled as i64, next_run_at, now, sid, task_id],
            )
            .map_err(|e| e.to_string())?;
            sid
        } else {
            let sid = Ulid::new().to_string();
            conn.execute(
                "INSERT INTO schedules (id, task_id, kind, config, enabled, next_run_at, created_at, updated_at, tenant_id)
                 VALUES (?1, ?2, 'recurring', ?3, ?4, ?5, ?6, ?6, COALESCE((SELECT tenant_id FROM tasks WHERE id = ?2), 'local'))",
                rusqlite::params![sid, task_id, sched_config_str, enabled as i64, next_run_at, now],
            )
            .map_err(|e| e.to_string())?;
            sid
        };

        // ── Ensure Pulse chat session exists (scoped to this project) ────
        let session_id: String = conn
            .query_row(
                "SELECT id FROM chat_sessions
                 WHERE agent_id = ?1
                   AND project_id = ?2
                   AND session_type = 'pulse'
                   AND tenant_id = COALESCE((SELECT tenant_id FROM projects WHERE id = ?2),
                                            (SELECT tenant_id FROM agents WHERE id = ?1),
                                            'local')",
                rusqlite::params![aid, pid],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| {
                let sid = Ulid::new().to_string();
                let _ = conn.execute(
                    "INSERT INTO chat_sessions (
                       id, agent_id, title, archived, session_type, parent_session_id, source_bus_message_id,
                       chain_depth, execution_state, finish_summary, terminal_error, created_at, updated_at, project_id, tenant_id
                     ) VALUES (?1, ?2, 'Pulse', 0, 'pulse', NULL, NULL, 0, NULL, NULL, NULL, ?3, ?3, ?4, COALESCE((SELECT tenant_id FROM agents WHERE id = ?2), 'local'))",
                    rusqlite::params![sid, aid, now, pid],
                );
                sid
            });

        let last_run_at: Option<String> = conn
            .query_row(
                "SELECT last_run_at FROM schedules
                  WHERE id = ?1
                    AND tenant_id = COALESCE((SELECT tenant_id FROM tasks WHERE id = ?2), 'local')",
                rusqlite::params![schedule_id, task_id],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        info!(agent_id = %aid, project_id = %pid, enabled = enabled, "Pulse configuration updated");

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

mod http {
    use super::*;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GetArgs {
        agent_id: String,
        project_id: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UpdateArgs {
        agent_id: String,
        project_id: String,
        content: String,
        schedule_config: RecurringConfig,
        enabled: bool,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("get_pulse_config", |ctx, args| async move {
            let a: GetArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = get_pulse_config_inner(a.agent_id, a.project_id, &ctx).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("update_pulse", |ctx, args| async move {
            let a: UpdateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = update_pulse_inner(
                a.agent_id,
                a.project_id,
                a.content,
                a.schedule_config,
                a.enabled,
                &ctx,
            )
            .await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
    }
}

pub use http::register as register_http;
