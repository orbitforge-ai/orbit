use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};
use ulid::Ulid;

use crate::db::repos::{sqlite::SqliteRepos, Repos};
use crate::db::DbPool;
use crate::executor::engine::{ExecutorTx, RunRequest};
use crate::models::schedule::{OneShotConfig, RecurringConfig, Schedule};
use crate::runtime_host::RuntimeHostHandle;
use crate::scheduler::converter::{compute_next, to_cron};
use crate::workflows::orchestrator::WorkflowOrchestrator;

/// The scheduler engine polls the database every 10 seconds and fires any
/// schedules whose next_run_at is due.
pub struct SchedulerEngine {
    db: DbPool,
    repos: Arc<dyn Repos>,
    executor_tx: ExecutorTx,
    host: RuntimeHostHandle,
    log_dir: PathBuf,
}

impl SchedulerEngine {
    pub fn new(
        db: DbPool,
        executor_tx: ExecutorTx,
        host: RuntimeHostHandle,
        log_dir: PathBuf,
    ) -> Self {
        let repos: Arc<dyn Repos> = Arc::new(SqliteRepos::new(db.clone()));
        Self::new_with_repos(db, repos, executor_tx, host, log_dir)
    }

    pub fn new_with_repos(
        db: DbPool,
        repos: Arc<dyn Repos>,
        executor_tx: ExecutorTx,
        host: RuntimeHostHandle,
        log_dir: PathBuf,
    ) -> Self {
        Self {
            db,
            repos,
            executor_tx,
            host,
            log_dir,
        }
    }

    pub async fn run(self) {
        info!("SchedulerEngine started");

        if let Err(e) = self.recover_orphans().await {
            warn!("orphan recovery failed: {}", e);
        }
        if let Err(e) = self.recover_workflow_orphans().await {
            warn!("workflow orphan recovery failed: {}", e);
        }
        if let Err(e) = self.recompute_next_runs().await {
            warn!("next_run_at recompute failed: {}", e);
        }

        let mut tick = interval(Duration::from_secs(10));

        loop {
            tick.tick().await;
            if let Err(e) = self.tick().await {
                error!("scheduler tick failed: {}", e);
            }
        }
    }

    async fn tick(&self) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        let due = self.repos.schedules().list_due(&now).await?;

        for schedule in due {
            match schedule.target_kind.as_str() {
                "task" => {
                    let task_id = match schedule.task_id.clone() {
                        Some(t) => t,
                        None => continue,
                    };
                    let task = match self.repos.tasks().get(&task_id).await? {
                        Some(t) if t.enabled => t,
                        _ => continue,
                    };

                    let run_id = Ulid::new().to_string();
                    let log_path = self.log_dir.join(format!("{}.log", run_id));
                    let log_path_str = log_path.to_string_lossy().to_string();
                    self.repos
                        .runs()
                        .create_scheduled_task_run(
                            &run_id,
                            &task,
                            &schedule.id,
                            &log_path_str,
                            &now,
                        )
                        .await?;

                    let _ = self.executor_tx.0.send(RunRequest {
                        run_id: run_id.clone(),
                        task,
                        schedule_id: Some(schedule.id.clone()),
                        _trigger: "scheduled".to_string(),
                        retry_count: 0,
                        _parent_run_id: None,
                        chain_depth: 0,
                    });

                    info!(
                        run_id = run_id,
                        schedule_id = schedule.id,
                        kind = schedule.kind,
                        "task run enqueued"
                    );
                }
                "workflow" => {
                    let workflow_id = match schedule.workflow_id.clone() {
                        Some(w) => w,
                        None => continue,
                    };

                    let trigger_data = serde_json::json!({
                        "schedule_id": schedule.id,
                        "fired_at": now,
                    });

                    let orchestrator = WorkflowOrchestrator::new_with_repos(
                        self.db.clone(),
                        self.repos.clone(),
                        self.host.clone(),
                    );
                    let workflow_id_log = workflow_id.clone();
                    let schedule_id_log = schedule.id.clone();
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

            self.advance_schedule(&schedule, &now).await?;
        }

        Ok(())
    }

    async fn advance_schedule(&self, schedule: &Schedule, fired_at: &str) -> Result<(), String> {
        match schedule.kind.as_str() {
            "recurring" => {
                if let Ok(cfg) = serde_json::from_value::<RecurringConfig>(schedule.config.clone())
                {
                    if let Ok(cron_expr) = to_cron(&cfg) {
                        let next = compute_next(&cron_expr);
                        self.repos
                            .schedules()
                            .mark_recurring_fired(&schedule.id, Some(&next), fired_at)
                            .await?;
                    }
                }
            }
            "one_shot" => {
                self.repos
                    .schedules()
                    .mark_one_shot_fired(&schedule.id, fired_at)
                    .await?;
            }
            _ => {}
        }

        Ok(())
    }

    async fn recover_workflow_orphans(&self) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        let err = "orphaned: process restarted";
        self.repos.workflow_runs().recover_orphans(err, &now).await
    }

    async fn recompute_next_runs(&self) -> Result<(), String> {
        let now_dt = chrono::Utc::now();
        let now = now_dt.to_rfc3339();
        let schedules = self.repos.schedules().list_needing_recompute(&now).await?;

        for schedule in schedules {
            match schedule.kind.as_str() {
                "recurring" => {
                    if let Ok(cfg) =
                        serde_json::from_value::<RecurringConfig>(schedule.config.clone())
                    {
                        if let Ok(cron_expr) = to_cron(&cfg) {
                            let next = compute_next(&cron_expr);
                            self.repos
                                .schedules()
                                .set_next_run_at(&schedule.id, &next, &now)
                                .await?;
                        }
                    }
                }
                "one_shot" => {
                    if let Ok(cfg) =
                        serde_json::from_value::<OneShotConfig>(schedule.config.clone())
                    {
                        // Parse the run_at datetime
                        if let Ok(run_at) = chrono::DateTime::parse_from_rfc3339(&cfg.run_at) {
                            let run_at_utc = run_at.with_timezone(&chrono::Utc);
                            if run_at_utc > now_dt {
                                // Still in the future — keep next_run_at as run_at
                                self.repos
                                    .schedules()
                                    .set_next_run_at(&schedule.id, &cfg.run_at, &now)
                                    .await?;
                            } else {
                                // Past — disable schedule (missed one-shot)
                                self.repos
                                    .schedules()
                                    .disable_missed_one_shot(&schedule.id, &now)
                                    .await?;
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    async fn recover_orphans(&self) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        let metadata = serde_json::json!({ "crash_reason": "orphaned" });
        self.repos.runs().recover_orphans(&now, metadata).await
    }
}
