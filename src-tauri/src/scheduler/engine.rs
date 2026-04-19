use std::path::PathBuf;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};
use ulid::Ulid;

use crate::db::DbPool;
use crate::executor::engine::{ExecutorTx, RunRequest};
use crate::models::schedule::{OneShotConfig, RecurringConfig};
use crate::models::task::Task;
use crate::scheduler::converter::{compute_next, to_cron};
use crate::workflows::orchestrator::WorkflowOrchestrator;

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
        Self {
            db,
            executor_tx,
            app,
            log_dir,
        }
    }

    pub async fn run(self) {
        info!("SchedulerEngine started");

        if let Err(e) = self.recover_orphans() {
            warn!("orphan recovery failed: {}", e);
        }
        if let Err(e) = self.recover_workflow_orphans() {
            warn!("workflow orphan recovery failed: {}", e);
        }
        if let Err(e) = self.recompute_next_runs() {
            warn!("next_run_at recompute failed: {}", e);
        }

        let mut tick = interval(Duration::from_secs(10));

        loop {
            tick.tick().await;
            if let Err(e) = self.tick() {
                error!("scheduler tick failed: {}", e);
            }
        }
    }

    fn tick(&self) -> Result<(), String> {
        let conn = self.db.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();

        let mut stmt = conn
            .prepare(
                "SELECT s.id, s.target_kind, s.task_id, s.workflow_id, s.kind, s.config
                 FROM schedules s
                 WHERE s.enabled = 1 AND s.next_run_at <= ?1",
            )
            .map_err(|e| e.to_string())?;

        type DueRow = (
            String,
            String,
            Option<String>,
            Option<String>,
            String,
            String,
        );
        let due: Vec<DueRow> = stmt
            .query_map(rusqlite::params![now], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        drop(stmt);

        for (schedule_id, target_kind, task_id, workflow_id, kind, config_str) in due {
            match target_kind.as_str() {
                "task" => {
                    let task_id = match task_id {
                        Some(t) => t,
                        None => continue,
                    };
                    let task = match load_task(&conn, &task_id) {
                        Some(t) if t.enabled => t,
                        _ => continue,
                    };

                    let run_id = Ulid::new().to_string();
                    let log_path = self.log_dir.join(format!("{}.log", run_id));

                    conn.execute(
                        "INSERT INTO runs (id, task_id, schedule_id, agent_id, state, trigger,
                                           log_path, retry_count, metadata, created_at)
                         VALUES (?1, ?2, ?3, ?4, 'pending', 'scheduled', ?5, 0, '{}', ?6)",
                        rusqlite::params![
                            run_id,
                            task_id,
                            schedule_id,
                            task.agent_id,
                            log_path.to_string_lossy().to_string(),
                            now
                        ],
                    )
                    .map_err(|e| e.to_string())?;

                    let _ = self.executor_tx.0.send(RunRequest {
                        run_id: run_id.clone(),
                        task,
                        schedule_id: Some(schedule_id.clone()),
                        _trigger: "scheduled".to_string(),
                        retry_count: 0,
                        _parent_run_id: None,
                        chain_depth: 0,
                    });

                    info!(
                        run_id = run_id,
                        schedule_id = schedule_id,
                        kind = kind,
                        "task run enqueued"
                    );
                }
                "workflow" => {
                    let workflow_id = match workflow_id {
                        Some(w) => w,
                        None => continue,
                    };

                    let trigger_data = serde_json::json!({
                        "schedule_id": schedule_id,
                        "fired_at": now,
                    });

                    let orchestrator =
                        WorkflowOrchestrator::new(self.db.clone(), self.app.clone());
                    let workflow_id_log = workflow_id.clone();
                    let schedule_id_log = schedule_id.clone();
                    tokio::spawn(async move {
                        match orchestrator
                            .start_run(workflow_id_log.clone(), "schedule", trigger_data)
                            .await
                        {
                            Ok(run) => info!(
                                run_id = run.id,
                                workflow_id = workflow_id_log,
                                schedule_id = schedule_id_log,
                                "workflow run started"
                            ),
                            Err(e) => warn!("scheduled workflow run failed to start: {}", e),
                        }
                    });
                }
                other => {
                    warn!("unknown schedule target_kind: {}", other);
                    continue;
                }
            }

            match kind.as_str() {
                "recurring" => {
                    if let Ok(cfg) = serde_json::from_str::<RecurringConfig>(&config_str) {
                        if let Ok(cron_expr) = to_cron(&cfg) {
                            let next = compute_next(&cron_expr);
                            conn
                .execute(
                  "UPDATE schedules SET next_run_at = ?1, last_run_at = ?2, updated_at = ?2 WHERE id = ?3",
                  rusqlite::params![next, now, schedule_id]
                )
                .ok();
                        }
                    }
                }
                "one_shot" => {
                    conn
            .execute(
              "UPDATE schedules SET enabled = 0, last_run_at = ?1, next_run_at = NULL, updated_at = ?1 WHERE id = ?2",
              rusqlite::params![now, schedule_id]
            )
            .ok();
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn recover_workflow_orphans(&self) -> Result<(), String> {
        let conn = self.db.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        let err = "orphaned: process restarted";

        conn.execute(
            "UPDATE workflow_runs
             SET status = 'failed', error = ?1, completed_at = ?2
             WHERE status IN ('queued', 'running')",
            rusqlite::params![err, now],
        )
        .map_err(|e| e.to_string())?;

        conn.execute(
            "UPDATE workflow_run_steps
             SET status = 'failed', error = COALESCE(error, ?1), completed_at = ?2
             WHERE status IN ('queued', 'running')",
            rusqlite::params![err, now],
        )
        .map_err(|e| e.to_string())?;

        Ok(())
    }

    fn recompute_next_runs(&self) -> Result<(), String> {
        let conn = self.db.get().map_err(|e| e.to_string())?;
        let now_dt = chrono::Utc::now();
        let now = now_dt.to_rfc3339();

        let mut stmt = conn
      .prepare(
        "SELECT id, kind, config FROM schedules WHERE enabled = 1 AND (next_run_at IS NULL OR next_run_at < ?1)"
      )
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
            match kind.as_str() {
                "recurring" => {
                    if let Ok(cfg) = serde_json::from_str::<RecurringConfig>(&config_str) {
                        if let Ok(cron_expr) = to_cron(&cfg) {
                            let next = compute_next(&cron_expr);
                            conn
                .execute(
                  "UPDATE schedules SET next_run_at = ?1, updated_at = ?2 WHERE id = ?3",
                  rusqlite::params![next, now, id]
                )
                .ok();
                        }
                    }
                }
                "one_shot" => {
                    if let Ok(cfg) = serde_json::from_str::<OneShotConfig>(&config_str) {
                        // Parse the run_at datetime
                        if let Ok(run_at) = chrono::DateTime::parse_from_rfc3339(&cfg.run_at) {
                            let run_at_utc = run_at.with_timezone(&chrono::Utc);
                            if run_at_utc > now_dt {
                                // Still in the future — keep next_run_at as run_at
                                conn
                  .execute(
                    "UPDATE schedules SET next_run_at = ?1, updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![cfg.run_at, now, id]
                  )
                  .ok();
                            } else {
                                // Past — disable schedule (missed one-shot)
                                conn
                  .execute(
                    "UPDATE schedules SET enabled = 0, next_run_at = NULL, updated_at = ?1 WHERE id = ?2",
                    rusqlite::params![now, id]
                  )
                  .ok();
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

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

fn load_task(conn: &rusqlite::Connection, task_id: &str) -> Option<Task> {
    conn.query_row(
        "SELECT id, name, description, kind, config, max_duration_seconds, max_retries,
                retry_delay_seconds, concurrency_policy, tags, agent_id,
                enabled, created_at, updated_at, project_id
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
                config: serde_json::from_str(&config_str).unwrap_or(serde_json::Value::Null),
                max_duration_seconds: row.get(5)?,
                max_retries: row.get(6)?,
                retry_delay_seconds: row.get(7)?,
                concurrency_policy: row.get(8)?,
                tags: serde_json::from_str(&tags_str).unwrap_or_default(),
                agent_id: row.get(10)?,
                enabled: row.get::<_, bool>(11)?,
                created_at: row.get(12)?,
                updated_at: row.get(13)?,
                project_id: row.get(14)?,
            })
        },
    )
    .ok()
}
