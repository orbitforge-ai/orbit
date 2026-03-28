use std::path::PathBuf;
use tokio::time::{interval, Duration};
use ulid::Ulid;

use crate::db::DbPool;
use crate::executor::engine::{ExecutorTx, RunRequest};
use crate::models::schedule::RecurringConfig;
use crate::models::task::Task;
use crate::scheduler::converter::to_cron;

/// The scheduler engine polls the database every 10 seconds and fires any
/// schedules whose next_run_at is due.
pub struct SchedulerEngine {
    db: DbPool,
    executor_tx: ExecutorTx,
    app: tauri::AppHandle,
    log_dir: PathBuf,
}

impl SchedulerEngine {
    pub fn new(
        db: DbPool,
        executor_tx: ExecutorTx,
        app: tauri::AppHandle,
        log_dir: PathBuf,
    ) -> Self {
        Self { db, executor_tx, app, log_dir }
    }

    pub async fn run(self) {
        tracing::info!("SchedulerEngine started");

        // On startup: recover orphaned runs and compute initial next_run_at values
        if let Err(e) = self.recover_orphans() {
            tracing::warn!("orphan recovery failed: {}", e);
        }
        if let Err(e) = self.recompute_next_runs() {
            tracing::warn!("next_run_at recompute failed: {}", e);
        }

        let mut tick = interval(Duration::from_secs(10));

        loop {
            tick.tick().await;
            if let Err(e) = self.tick() {
                tracing::error!("scheduler tick failed: {}", e);
            }
        }
    }

    /// Check for due schedules and enqueue runs.
    fn tick(&self) -> Result<(), String> {
        let conn = self.db.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();

        // Find all enabled schedules where next_run_at <= now
        let mut stmt = conn
            .prepare(
                "SELECT s.id, s.task_id, s.kind, s.config, s.enabled, s.next_run_at, s.last_run_at, s.created_at, s.updated_at
                 FROM schedules s
                 WHERE s.enabled = 1 AND s.next_run_at <= ?1",
            )
            .map_err(|e| e.to_string())?;

        let due: Vec<(String, String, String, String)> = stmt
            .query_map(rusqlite::params![now], |row| {
                Ok((
                    row.get::<_, String>(0)?, // schedule id
                    row.get::<_, String>(1)?, // task_id
                    row.get::<_, String>(2)?, // kind
                    row.get::<_, String>(3)?, // config
                ))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        drop(stmt);

        for (schedule_id, task_id, _kind, config_str) in due {
            // Load the task
            let task = match load_task(&conn, &task_id) {
                Some(t) if t.enabled => t,
                _ => continue,
            };

            // Create a Run record
            let run_id = Ulid::new().to_string();
            let log_path = self.log_dir.join(format!("{}.log", run_id));

            conn.execute(
                "INSERT INTO runs (id, task_id, schedule_id, agent_id, state, trigger, log_path, retry_count, metadata, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'pending', 'scheduled', ?5, 0, '{}', ?6)",
                rusqlite::params![
                    run_id,
                    task_id,
                    schedule_id,
                    task.agent_id,
                    log_path.to_string_lossy().to_string(),
                    now,
                ],
            )
            .map_err(|e| e.to_string())?;

            // Enqueue to executor
            let _ = self.executor_tx.0.send(RunRequest {
                run_id: run_id.clone(),
                task,
                schedule_id: Some(schedule_id.clone()),
                trigger: "scheduled".to_string(),
            });

            // Advance next_run_at for recurring schedules
            if let Ok(cfg) = serde_json::from_str::<RecurringConfig>(&config_str) {
                if let Ok(cron_expr) = to_cron(&cfg) {
                    let next = compute_next(&cron_expr);
                    conn.execute(
                        "UPDATE schedules SET next_run_at = ?1, last_run_at = ?2, updated_at = ?2 WHERE id = ?3",
                        rusqlite::params![next, now, schedule_id],
                    )
                    .ok();
                }
            }

            tracing::info!(run_id = run_id, schedule_id = schedule_id, "run enqueued");
        }

        Ok(())
    }

    /// On startup, recompute next_run_at for all recurring schedules that have
    /// no next_run_at or whose next_run_at is in the past.
    fn recompute_next_runs(&self) -> Result<(), String> {
        let conn = self.db.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();

        let mut stmt = conn
            .prepare("SELECT id, kind, config FROM schedules WHERE enabled = 1 AND (next_run_at IS NULL OR next_run_at < ?1)")
            .map_err(|e| e.to_string())?;

        let rows: Vec<(String, String, String)> = stmt
            .query_map(rusqlite::params![now], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        drop(stmt);

        for (id, kind, config_str) in rows {
            if kind == "recurring" {
                if let Ok(cfg) = serde_json::from_str::<RecurringConfig>(&config_str) {
                    if let Ok(cron_expr) = to_cron(&cfg) {
                        let next = compute_next(&cron_expr);
                        conn.execute(
                            "UPDATE schedules SET next_run_at = ?1, updated_at = ?2 WHERE id = ?3",
                            rusqlite::params![next, now, id],
                        )
                        .ok();
                    }
                }
            }
        }

        Ok(())
    }

    /// Mark any runs still in running/queued state as failed (orphans from a previous crash).
    fn recover_orphans(&self) -> Result<(), String> {
        let conn = self.db.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        let metadata = serde_json::json!({ "crash_reason": "orphaned" }).to_string();

        conn.execute(
            "UPDATE runs SET state = 'failure', finished_at = ?1, metadata = ?2
             WHERE state IN ('running', 'queued')",
            rusqlite::params![now, metadata],
        )
        .map_err(|e| e.to_string())?;

        Ok(())
    }
}

/// Compute the next run time after now for a cron expression.
/// Uses a simple approximation: advances by the schedule's smallest unit.
fn compute_next(cron_expr: &str) -> String {
    // For a production-quality impl, use cron parsing.
    // For M1, advance by 1 minute from now and let the tick catch up.
    // TODO M2: integrate proper cron-next computation.
    let parts: Vec<&str> = cron_expr.split_whitespace().collect();
    let advance_minutes: i64 = if parts.len() >= 2 && parts[1].starts_with("*/") {
        parts[1][2..].parse().unwrap_or(1)
    } else {
        1440 // default: daily
    };

    let next = chrono::Utc::now() + chrono::Duration::minutes(advance_minutes);
    next.to_rfc3339()
}

fn load_task(conn: &rusqlite::Connection, task_id: &str) -> Option<Task> {
    conn.query_row(
        "SELECT id, name, description, kind, config, max_duration_seconds, max_retries,
                retry_delay_seconds, concurrency_policy, tags, agent_id, session_id,
                enabled, created_at, updated_at
         FROM tasks WHERE id = ?1",
        rusqlite::params![task_id],
        |row| {
            let config_str: String = row.get(4)?;
            let tags_str: String = row.get(9)?;
            Ok(Task {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                kind: row.get(3)?,
                config: serde_json::from_str(&config_str)
                    .unwrap_or(serde_json::Value::Null),
                max_duration_seconds: row.get(5)?,
                max_retries: row.get(6)?,
                retry_delay_seconds: row.get(7)?,
                concurrency_policy: row.get(8)?,
                tags: serde_json::from_str(&tags_str).unwrap_or_default(),
                agent_id: row.get(10)?,
                session_id: row.get(11)?,
                enabled: row.get::<_, bool>(12)?,
                created_at: row.get(13)?,
                updated_at: row.get(14)?,
            })
        },
    )
    .ok()
}
