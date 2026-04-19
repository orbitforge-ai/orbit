use serde::Serialize;
use serde_json::{json, Value};
use ulid::Ulid;

use crate::db::DbPool;
use crate::executor::engine::RunRequest;
use crate::executor::llm_provider::ToolDefinition;
use crate::executor::workspace;
use crate::models::chat::ChatSession;
use crate::models::schedule::{OneShotConfig, RecurringConfig, Schedule};
use crate::models::task::Task;
use crate::scheduler::converter::{compute_next, next_n_runs, to_cron};

use super::{context::ToolExecutionContext, ToolHandler};

const PULSE_TAG: &str = "\"pulse\"";
const DEFAULT_PREVIEW_COUNT: usize = 5;
const MAX_PREVIEW_COUNT: usize = 20;

pub struct ScheduleTool;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleListItem {
    schedule_id: String,
    task_id: String,
    task_name: String,
    kind: String,
    enabled: bool,
    next_run_at: Option<String>,
    last_run_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PulseRunResult {
    status: String,
    run_id: String,
    task_id: String,
    schedule_id: Option<String>,
    session_id: Option<String>,
}

#[derive(Debug, Clone)]
struct OwnedTask {
    task: Task,
    is_pulse: bool,
}

#[derive(Debug, Clone)]
struct OwnedSchedule {
    schedule: Schedule,
    is_pulse: bool,
}

#[async_trait::async_trait]
impl ToolHandler for ScheduleTool {
    fn name(&self) -> &'static str {
        "schedule"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Inspect and manage recurring automation for this agent. Supports task schedules and the agent's pulse configuration.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "list",
                            "create",
                            "update",
                            "toggle",
                            "delete",
                            "preview",
                            "pulse_get",
                            "pulse_set",
                            "pulse_run"
                        ],
                        "description": "Scheduling action to perform"
                    },
                    "schedule_id": {
                        "type": "string",
                        "description": "Schedule ID for update, toggle, or delete"
                    },
                    "task_id": {
                        "type": "string",
                        "description": "Existing task ID owned by the current agent"
                    },
                    "enabled": {
                        "type": "boolean",
                        "description": "Enable or disable a schedule"
                    },
                    "kind": {
                        "type": "string",
                        "enum": ["recurring", "one_shot"],
                        "description": "Schedule kind. Defaults to recurring."
                    },
                    "config": {
                        "type": "object",
                        "description": "Schedule config payload matching Orbit's existing schedule schema"
                    },
                    "preview_count": {
                        "type": "integer",
                        "description": "How many future runs to preview. Defaults to 5 and is capped at 20."
                    },
                    "pulse_content": {
                        "type": "string",
                        "description": "Prompt content for the agent's recurring pulse"
                    },
                    "pulse_enabled": {
                        "type": "boolean",
                        "description": "Whether pulse scheduling should be active"
                    },
                    "pulse_schedule": {
                        "type": "object",
                        "description": "Recurring schedule config for pulse"
                    }
                },
                "required": ["action"]
            }),
        }
    }

    async fn execute(
        &self,
        ctx: &ToolExecutionContext,
        input: &serde_json::Value,
        _app: &tauri::AppHandle,
        _run_id: &str,
    ) -> Result<(String, bool), String> {
        let db = ctx.db.as_ref().ok_or("schedule: no database available")?;
        let action = input["action"]
            .as_str()
            .ok_or("schedule: missing 'action' field")?;

        let result = match action {
            "list" => serde_json::to_string_pretty(&list_owned_schedules(db, &ctx.agent_id).await?)
                .map_err(|e| format!("schedule: failed to serialize result: {}", e))?,
            "create" => {
                let task_id = require_string(input, "task_id", "create")?;
                let config = input
                    .get("config")
                    .ok_or("schedule: create requires 'config'")?;
                let kind = input["kind"].as_str().unwrap_or("recurring");
                let schedule = create_owned_schedule(db, ctx, task_id, kind, config).await?;
                serde_json::to_string_pretty(&schedule)
                    .map_err(|e| format!("schedule: failed to serialize result: {}", e))?
            }
            "update" => {
                let schedule_id = require_string(input, "schedule_id", "update")?;
                let config = input.get("config");
                let enabled = input.get("enabled").and_then(Value::as_bool);
                let schedule = update_owned_schedule(db, ctx, schedule_id, config, enabled).await?;
                serde_json::to_string_pretty(&schedule)
                    .map_err(|e| format!("schedule: failed to serialize result: {}", e))?
            }
            "toggle" => {
                let schedule_id = require_string(input, "schedule_id", "toggle")?;
                let enabled = input["enabled"]
                    .as_bool()
                    .ok_or("schedule: toggle requires 'enabled'")?;
                let schedule = toggle_owned_schedule(db, ctx, schedule_id, enabled).await?;
                serde_json::to_string_pretty(&schedule)
                    .map_err(|e| format!("schedule: failed to serialize result: {}", e))?
            }
            "delete" => {
                let schedule_id = require_string(input, "schedule_id", "delete")?;
                let deleted = delete_owned_schedule(db, ctx, schedule_id).await?;
                serde_json::to_string_pretty(&json!({
                    "status": "deleted",
                    "schedule": deleted,
                }))
                .map_err(|e| format!("schedule: failed to serialize result: {}", e))?
            }
            "preview" => {
                let kind = input["kind"].as_str().unwrap_or("recurring");
                let config = input
                    .get("config")
                    .ok_or("schedule: preview requires 'config'")?;
                let preview_count = input["preview_count"]
                    .as_u64()
                    .unwrap_or(DEFAULT_PREVIEW_COUNT as u64)
                    .min(MAX_PREVIEW_COUNT as u64) as usize;
                let preview = preview_schedule(kind, config, preview_count)?;
                serde_json::to_string_pretty(&preview)
                    .map_err(|e| format!("schedule: failed to serialize result: {}", e))?
            }
            "pulse_get" => {
                let pulse = get_pulse_config(db, &ctx.agent_id).await?;
                serde_json::to_string_pretty(&pulse)
                    .map_err(|e| format!("schedule: failed to serialize result: {}", e))?
            }
            "pulse_set" => {
                let pulse_content = input["pulse_content"]
                    .as_str()
                    .ok_or("schedule: pulse_set requires 'pulse_content'")?;
                let pulse_enabled = input["pulse_enabled"]
                    .as_bool()
                    .ok_or("schedule: pulse_set requires 'pulse_enabled'")?;
                let pulse_schedule_value = input
                    .get("pulse_schedule")
                    .ok_or("schedule: pulse_set requires 'pulse_schedule'")?;
                let pulse_schedule: RecurringConfig =
                    serde_json::from_value(pulse_schedule_value.clone())
                        .map_err(|e| format!("schedule: invalid pulse_schedule: {}", e))?;
                let pulse =
                    set_pulse_config(db, ctx, pulse_content, pulse_schedule, pulse_enabled).await?;
                serde_json::to_string_pretty(&pulse)
                    .map_err(|e| format!("schedule: failed to serialize result: {}", e))?
            }
            "pulse_run" => {
                let result = trigger_pulse_run(db, ctx).await?;
                serde_json::to_string_pretty(&result)
                    .map_err(|e| format!("schedule: failed to serialize result: {}", e))?
            }
            other => return Err(format!("schedule: unknown action '{}'", other)),
        };

        Ok((result, false))
    }
}

fn require_string<'a>(input: &'a Value, field: &str, action: &str) -> Result<&'a str, String> {
    input[field]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("schedule: {} requires '{}'", action, field))
}

async fn list_owned_schedules(
    db: &DbPool,
    agent_id: &str,
) -> Result<Vec<ScheduleListItem>, String> {
    let pool = db.0.clone();
    let agent_id = agent_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<Vec<ScheduleListItem>, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT s.id, s.task_id, t.name, s.kind, s.enabled, s.next_run_at, s.last_run_at
                 FROM schedules s
                 INNER JOIN tasks t ON t.id = s.task_id
                 WHERE t.agent_id = ?1
                   AND t.tags NOT LIKE '%\"pulse\"%'
                 ORDER BY s.created_at DESC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(rusqlite::params![agent_id], |row| {
                Ok(ScheduleListItem {
                    schedule_id: row.get(0)?,
                    task_id: row.get(1)?,
                    task_name: row.get(2)?,
                    kind: row.get(3)?,
                    enabled: row.get::<_, bool>(4)?,
                    next_run_at: row.get(5)?,
                    last_run_at: row.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|row| row.ok())
            .collect();

        Ok(rows)
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn create_owned_schedule(
    db: &DbPool,
    ctx: &ToolExecutionContext,
    task_id: &str,
    kind: &str,
    config: &Value,
) -> Result<Schedule, String> {
    let owned_task = load_owned_task(db, &ctx.agent_id, task_id).await?;
    if owned_task.is_pulse {
        return Err(
            "schedule: pulse-backed tasks must be managed through pulse_get/pulse_set/pulse_run"
                .to_string(),
        );
    }

    let kind = normalize_schedule_kind(kind)?;
    let config_json = validate_schedule_config(&kind, config)?;
    let next_run_at = compute_next_run_at_for_kind(&kind, &config_json, true)?;
    let schedule =
        insert_schedule_record(db, task_id, &kind, &config_json, true, next_run_at).await?;
    sync_schedule_upsert(ctx, &schedule);
    Ok(schedule)
}

async fn update_owned_schedule(
    db: &DbPool,
    ctx: &ToolExecutionContext,
    schedule_id: &str,
    config: Option<&Value>,
    enabled: Option<bool>,
) -> Result<Schedule, String> {
    let owned = load_owned_schedule(db, &ctx.agent_id, schedule_id).await?;
    ensure_not_pulse_schedule(&owned)?;

    let next_config = match config {
        Some(config) => validate_schedule_config(&owned.schedule.kind, config)?,
        None => owned.schedule.config.clone(),
    };
    let next_enabled = enabled.unwrap_or(owned.schedule.enabled);
    let next_run_at =
        compute_next_run_at_for_kind(&owned.schedule.kind, &next_config, next_enabled)?;
    let schedule = update_schedule_record(
        db,
        &owned.schedule.id,
        &next_config,
        next_enabled,
        next_run_at,
    )
    .await?;
    sync_schedule_upsert(ctx, &schedule);
    Ok(schedule)
}

async fn toggle_owned_schedule(
    db: &DbPool,
    ctx: &ToolExecutionContext,
    schedule_id: &str,
    enabled: bool,
) -> Result<Schedule, String> {
    let owned = load_owned_schedule(db, &ctx.agent_id, schedule_id).await?;
    ensure_not_pulse_schedule(&owned)?;
    let next_run_at =
        compute_next_run_at_for_kind(&owned.schedule.kind, &owned.schedule.config, enabled)?;
    let schedule = update_schedule_record(
        db,
        &owned.schedule.id,
        &owned.schedule.config,
        enabled,
        next_run_at,
    )
    .await?;
    sync_schedule_upsert(ctx, &schedule);
    Ok(schedule)
}

async fn delete_owned_schedule(
    db: &DbPool,
    ctx: &ToolExecutionContext,
    schedule_id: &str,
) -> Result<Schedule, String> {
    let owned = load_owned_schedule(db, &ctx.agent_id, schedule_id).await?;
    ensure_not_pulse_schedule(&owned)?;
    delete_schedule_record(db, &owned.schedule.id).await?;
    sync_schedule_delete(ctx, &owned.schedule.id);
    Ok(owned.schedule)
}

fn preview_schedule(kind: &str, config: &Value, count: usize) -> Result<Value, String> {
    let kind = normalize_schedule_kind(kind)?;
    let config = validate_schedule_config(&kind, config)?;
    match kind.as_str() {
        "recurring" => {
            let cfg: RecurringConfig = serde_json::from_value(config)
                .map_err(|e| format!("schedule: invalid recurring config: {}", e))?;
            Ok(json!({
                "kind": "recurring",
                "runs": next_n_runs(&cfg, count),
            }))
        }
        "one_shot" => {
            let cfg: OneShotConfig = serde_json::from_value(config)
                .map_err(|e| format!("schedule: invalid one_shot config: {}", e))?;
            Ok(json!({
                "kind": "one_shot",
                "runs": [cfg.run_at],
            }))
        }
        _ => unreachable!(),
    }
}

async fn get_pulse_config(
    db: &DbPool,
    agent_id: &str,
) -> Result<crate::commands::pulse::PulseConfig, String> {
    let pool = db.0.clone();
    let agent_id = agent_id.to_string();
    let content = workspace::read_workspace_file(&agent_id, "pulse.md").unwrap_or_default();

    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;

        let pulse_task: Option<(String, bool)> = conn
            .query_row(
                "SELECT id, enabled FROM tasks WHERE agent_id = ?1 AND tags LIKE ?2",
                rusqlite::params![agent_id.clone(), format!("%{}%", PULSE_TAG)],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)),
            )
            .ok();

        let (task_id, schedule_id, enabled, schedule, next_run_at, last_run_at) =
            if let Some((task_id, _)) = pulse_task {
                let sched: Option<(String, String, bool, Option<String>, Option<String>)> = conn
                    .query_row(
                        "SELECT id, config, enabled, next_run_at, last_run_at
                         FROM schedules WHERE task_id = ?1",
                        rusqlite::params![task_id.clone()],
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

                if let Some((schedule_id, config_str, enabled, next_run_at, last_run_at)) = sched {
                    let schedule = serde_json::from_str::<RecurringConfig>(&config_str).ok();
                    (
                        Some(task_id),
                        Some(schedule_id),
                        enabled,
                        schedule,
                        next_run_at,
                        last_run_at,
                    )
                } else {
                    (Some(task_id), None, false, None, None, None)
                }
            } else {
                (None, None, false, None, None, None)
            };

        let session_id: Option<String> = conn
            .query_row(
                "SELECT id FROM chat_sessions WHERE agent_id = ?1 AND session_type = 'pulse'",
                rusqlite::params![agent_id],
                |row| row.get(0),
            )
            .ok();

        Ok(crate::commands::pulse::PulseConfig {
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

async fn set_pulse_config(
    db: &DbPool,
    ctx: &ToolExecutionContext,
    pulse_content: &str,
    pulse_schedule: RecurringConfig,
    pulse_enabled: bool,
) -> Result<crate::commands::pulse::PulseConfig, String> {
    workspace::write_workspace_file(&ctx.agent_id, "pulse.md", pulse_content)?;

    let pool = db.0.clone();
    let agent_id = ctx.agent_id.clone();
    let pulse_content = pulse_content.to_string();
    let pulse_content_for_db = pulse_content.clone();
    let pulse_schedule_clone = pulse_schedule.clone();
    let pulse_enabled_i64 = if pulse_enabled { 1 } else { 0 };

    let (task, schedule, session): (Task, Schedule, ChatSession) =
        tokio::task::spawn_blocking(move || -> Result<(Task, Schedule, ChatSession), String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            let now = chrono::Utc::now().to_rfc3339();

            let existing_task_id: Option<String> = conn
                .query_row(
                    "SELECT id FROM tasks WHERE agent_id = ?1 AND tags LIKE ?2",
                    rusqlite::params![agent_id.clone(), format!("%{}%", PULSE_TAG)],
                    |row| row.get(0),
                )
                .ok();

            let task_config = json!({ "goal": pulse_content_for_db });
            let task_config_str = task_config.to_string();
            let agent_name: String = conn
                .query_row(
                    "SELECT name FROM agents WHERE id = ?1",
                    rusqlite::params![agent_id.clone()],
                    |row| row.get(0),
                )
                .unwrap_or_else(|_| agent_id.chars().take(20).collect());

            let task_id = if let Some(task_id) = existing_task_id {
                conn.execute(
                    "UPDATE tasks
                     SET config = ?1, name = ?2, enabled = 1, updated_at = ?3
                     WHERE id = ?4",
                    rusqlite::params![
                        task_config_str,
                        format!("[Pulse] {}", agent_name),
                        now,
                        task_id
                    ],
                )
                .map_err(|e| e.to_string())?;
                task_id
            } else {
                let task_id = Ulid::new().to_string();
                conn.execute(
                    "INSERT INTO tasks (
                        id, name, description, kind, config, max_duration_seconds,
                        max_retries, retry_delay_seconds, concurrency_policy, tags,
                        agent_id, enabled, created_at, updated_at
                    ) VALUES (?1, ?2, 'Automated pulse schedule', 'agent_loop', ?3, 7200, 0, 60, 'skip', '[\"pulse\"]', ?4, 1, ?5, ?5)",
                    rusqlite::params![
                        task_id,
                        format!("[Pulse] {}", agent_name),
                        task_config_str,
                        agent_id.clone(),
                        now
                    ],
                )
                .map_err(|e| e.to_string())?;
                task_id
            };

            let sched_config_str =
                serde_json::to_string(&pulse_schedule_clone).map_err(|e| e.to_string())?;
            let next_run_at =
                compute_next_run_at_for_kind("recurring", &serde_json::to_value(&pulse_schedule_clone).map_err(|e| e.to_string())?, pulse_enabled)?;

            let existing_schedule_id: Option<String> = conn
                .query_row(
                    "SELECT id FROM schedules WHERE task_id = ?1",
                    rusqlite::params![task_id.clone()],
                    |row| row.get(0),
                )
                .ok();

            let schedule_id = if let Some(schedule_id) = existing_schedule_id {
                conn.execute(
                    "UPDATE schedules
                     SET config = ?1, enabled = ?2, next_run_at = ?3, updated_at = ?4
                     WHERE id = ?5",
                    rusqlite::params![
                        sched_config_str,
                        pulse_enabled_i64,
                        next_run_at,
                        now,
                        schedule_id
                    ],
                )
                .map_err(|e| e.to_string())?;
                schedule_id
            } else {
                let schedule_id = Ulid::new().to_string();
                conn.execute(
                    "INSERT INTO schedules (
                        id, task_id, kind, config, enabled, next_run_at, created_at, updated_at
                     ) VALUES (?1, ?2, 'recurring', ?3, ?4, ?5, ?6, ?6)",
                    rusqlite::params![
                        schedule_id,
                        task_id.clone(),
                        sched_config_str,
                        pulse_enabled_i64,
                        next_run_at,
                        now
                    ],
                )
                .map_err(|e| e.to_string())?;
                schedule_id
            };

            let session_id: Option<String> = conn
                .query_row(
                    "SELECT id FROM chat_sessions WHERE agent_id = ?1 AND session_type = 'pulse'",
                    rusqlite::params![agent_id.clone()],
                    |row| row.get(0),
                )
                .ok();
            let session = if let Some(session_id) = session_id {
                conn.query_row(
                    "SELECT id, agent_id, title, archived, session_type, parent_session_id, source_bus_message_id,
                            chain_depth, execution_state, finish_summary, terminal_error,
                            created_at, updated_at, project_id, worktree_name, worktree_branch, worktree_path
                     FROM chat_sessions WHERE id = ?1",
                    rusqlite::params![session_id],
                    parse_chat_session_row,
                )
                .map_err(|e| e.to_string())?
            } else {
                let session_id = Ulid::new().to_string();
                conn.execute(
                    "INSERT INTO chat_sessions (
                        id, agent_id, title, archived, session_type, parent_session_id, source_bus_message_id,
                        chain_depth, execution_state, finish_summary, terminal_error, created_at, updated_at,
                        worktree_name, worktree_branch, worktree_path
                     ) VALUES (?1, ?2, 'Pulse', 0, 'pulse', NULL, NULL, 0, NULL, NULL, NULL, ?3, ?3, NULL, NULL, NULL)",
                    rusqlite::params![session_id, agent_id.clone(), now],
                )
                .map_err(|e| e.to_string())?;
                conn.query_row(
                    "SELECT id, agent_id, title, archived, session_type, parent_session_id, source_bus_message_id,
                            chain_depth, execution_state, finish_summary, terminal_error,
                            created_at, updated_at, project_id, worktree_name, worktree_branch, worktree_path
                     FROM chat_sessions WHERE id = ?1",
                    rusqlite::params![session_id],
                    parse_chat_session_row,
                )
                .map_err(|e| e.to_string())?
            };

            let task = conn
                .query_row(
                    "SELECT id, name, description, kind, config, max_duration_seconds, max_retries,
                            retry_delay_seconds, concurrency_policy, tags, agent_id, enabled,
                            created_at, updated_at, project_id
                     FROM tasks WHERE id = ?1",
                    rusqlite::params![task_id],
                    parse_task_row,
                )
                .map_err(|e| e.to_string())?;
            let schedule = conn
                .query_row(
                    "SELECT id, task_id, workflow_id, target_kind, kind, config, enabled,
                            next_run_at, last_run_at, created_at, updated_at
                     FROM schedules WHERE id = ?1",
                    rusqlite::params![schedule_id],
                    parse_schedule_row,
                )
                .map_err(|e| e.to_string())?;

            Ok((task, schedule, session))
        })
        .await
        .map_err(|e| e.to_string())??;

    sync_task_upsert(ctx, &task);
    sync_schedule_upsert(ctx, &schedule);
    sync_chat_session_upsert(ctx, &session);

    Ok(crate::commands::pulse::PulseConfig {
        enabled: pulse_enabled,
        content: pulse_content.to_string(),
        schedule: Some(pulse_schedule),
        task_id: Some(task.id),
        schedule_id: Some(schedule.id),
        session_id: Some(session.id),
        next_run_at: schedule.next_run_at,
        last_run_at: schedule.last_run_at,
    })
}

async fn trigger_pulse_run(
    db: &DbPool,
    ctx: &ToolExecutionContext,
) -> Result<PulseRunResult, String> {
    let executor_tx = ctx
        .executor_tx
        .as_ref()
        .ok_or("schedule: executor channel not available")?;
    let pulse = get_pulse_config(db, &ctx.agent_id).await?;
    let task_id = pulse
        .task_id
        .clone()
        .ok_or("schedule: pulse is not configured for this agent")?;

    let task = load_owned_task(db, &ctx.agent_id, &task_id).await?.task;
    let run_id = Ulid::new().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let pool = db.0.clone();
    let log_path = format!(
        "{}/logs/{}.log",
        crate::data_dir().to_string_lossy(),
        run_id
    );

    let run_id_for_db = run_id.clone();
    let schedule_id = pulse.schedule_id.clone();
    let agent_id = ctx.agent_id.clone();
    let project_id = task.project_id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO runs (
                id, task_id, schedule_id, agent_id, state, trigger, log_path, retry_count, metadata, project_id, created_at
             ) VALUES (?1, ?2, ?3, ?4, 'pending', 'manual', ?5, 0, '{}', ?6, ?7)",
            rusqlite::params![
                run_id_for_db,
                task_id,
                schedule_id,
                agent_id,
                log_path,
                project_id,
                now
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;

    executor_tx
        .send(RunRequest {
            run_id: run_id.clone(),
            task: task.clone(),
            schedule_id: pulse.schedule_id.clone(),
            _trigger: "manual".to_string(),
            retry_count: 0,
            _parent_run_id: None,
            chain_depth: 0,
        })
        .map_err(|e| format!("schedule: failed to enqueue pulse run: {}", e))?;

    Ok(PulseRunResult {
        status: "triggered".to_string(),
        run_id,
        task_id: task.id,
        schedule_id: pulse.schedule_id,
        session_id: pulse.session_id,
    })
}

async fn load_owned_task(db: &DbPool, agent_id: &str, task_id: &str) -> Result<OwnedTask, String> {
    let pool = db.0.clone();
    let agent_id = agent_id.to_string();
    let task_id = task_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<OwnedTask, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT id, name, description, kind, config, max_duration_seconds, max_retries,
                    retry_delay_seconds, concurrency_policy, tags, agent_id, enabled,
                    created_at, updated_at, project_id
             FROM tasks
             WHERE id = ?1 AND agent_id = ?2",
            rusqlite::params![task_id.clone(), agent_id],
            |row| {
                let task = parse_task_row(row)?;
                Ok(OwnedTask {
                    is_pulse: task.tags.iter().any(|tag| tag == "pulse"),
                    task,
                })
            },
        )
        .map_err(|_| format!("schedule: task '{}' not found for this agent", task_id))
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn load_owned_schedule(
    db: &DbPool,
    agent_id: &str,
    schedule_id: &str,
) -> Result<OwnedSchedule, String> {
    let pool = db.0.clone();
    let agent_id = agent_id.to_string();
    let schedule_id = schedule_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<OwnedSchedule, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT s.id, s.task_id, s.workflow_id, s.target_kind, s.kind, s.config, s.enabled,
                    s.next_run_at, s.last_run_at, s.created_at, s.updated_at,
                    t.id, t.name, t.description, t.kind, t.config, t.max_duration_seconds, t.max_retries,
                    t.retry_delay_seconds, t.concurrency_policy, t.tags, t.agent_id, t.enabled, t.created_at, t.updated_at, t.project_id
             FROM schedules s
             INNER JOIN tasks t ON t.id = s.task_id
             WHERE s.id = ?1 AND t.agent_id = ?2",
            rusqlite::params![schedule_id.clone(), agent_id],
            |row| {
                let schedule = Schedule {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    workflow_id: row.get(2)?,
                    target_kind: row.get(3)?,
                    kind: row.get(4)?,
                    config: serde_json::from_str::<Value>(&row.get::<_, String>(5)?)
                        .unwrap_or(Value::Null),
                    enabled: row.get::<_, bool>(6)?,
                    next_run_at: row.get(7)?,
                    last_run_at: row.get(8)?,
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                };
                let tags: Vec<String> =
                    serde_json::from_str(&row.get::<_, String>(20)?).unwrap_or_default();
                let _: String = row.get(11)?;
                let _: String = row.get(12)?;
                let _: Option<String> = row.get(13)?;
                let _: String = row.get(14)?;
                let _: String = row.get(15)?;
                let _: i64 = row.get(16)?;
                let _: i64 = row.get(17)?;
                let _: i64 = row.get(18)?;
                let _: String = row.get(19)?;
                let _: Option<String> = row.get(21)?;
                let _: bool = row.get(22)?;
                let _: String = row.get(23)?;
                let _: String = row.get(24)?;
                let _: Option<String> = row.get(25)?;
                Ok(OwnedSchedule {
                    is_pulse: tags.iter().any(|tag| tag == "pulse"),
                    schedule,
                })
            },
        )
        .map_err(|_| format!("schedule: schedule '{}' not found for this agent", schedule_id))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn ensure_not_pulse_schedule(owned: &OwnedSchedule) -> Result<(), String> {
    if owned.is_pulse {
        Err("schedule: pulse-backed schedules must be managed through pulse_get/pulse_set/pulse_run".to_string())
    } else {
        Ok(())
    }
}

fn normalize_schedule_kind(kind: &str) -> Result<String, String> {
    match kind {
        "recurring" | "one_shot" => Ok(kind.to_string()),
        other => Err(format!(
            "schedule: invalid kind '{}'; expected recurring or one_shot",
            other
        )),
    }
}

fn validate_schedule_config(kind: &str, config: &Value) -> Result<Value, String> {
    match kind {
        "recurring" => {
            let cfg: RecurringConfig = serde_json::from_value(config.clone())
                .map_err(|e| format!("schedule: invalid recurring config: {}", e))?;
            serde_json::to_value(cfg).map_err(|e| e.to_string())
        }
        "one_shot" => {
            let cfg: OneShotConfig = serde_json::from_value(config.clone())
                .map_err(|e| format!("schedule: invalid one_shot config: {}", e))?;
            serde_json::to_value(cfg).map_err(|e| e.to_string())
        }
        _ => Err(format!("schedule: unsupported kind '{}'", kind)),
    }
}

fn compute_next_run_at_for_kind(
    kind: &str,
    config: &Value,
    enabled: bool,
) -> Result<Option<String>, String> {
    if !enabled {
        return Ok(None);
    }

    match kind {
        "recurring" => {
            let cfg: RecurringConfig = serde_json::from_value(config.clone())
                .map_err(|e| format!("schedule: invalid recurring config: {}", e))?;
            let cron =
                to_cron(&cfg).map_err(|e| format!("schedule: invalid recurring config: {}", e))?;
            Ok(Some(compute_next(&cron)))
        }
        "one_shot" => {
            let cfg: OneShotConfig = serde_json::from_value(config.clone())
                .map_err(|e| format!("schedule: invalid one_shot config: {}", e))?;
            let run_at = chrono::DateTime::parse_from_rfc3339(&cfg.run_at)
                .map_err(|e| format!("schedule: invalid one_shot run_at: {}", e))?;
            let run_at_utc = run_at.with_timezone(&chrono::Utc);
            if run_at_utc > chrono::Utc::now() {
                Ok(Some(cfg.run_at))
            } else {
                Ok(None)
            }
        }
        _ => Err(format!("schedule: unsupported kind '{}'", kind)),
    }
}

async fn insert_schedule_record(
    db: &DbPool,
    task_id: &str,
    kind: &str,
    config: &Value,
    enabled: bool,
    next_run_at: Option<String>,
) -> Result<Schedule, String> {
    let pool = db.0.clone();
    let task_id = task_id.to_string();
    let kind = kind.to_string();
    let config = config.clone();
    tokio::task::spawn_blocking(move || -> Result<Schedule, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let id = Ulid::new().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let config_str = serde_json::to_string(&config).map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO schedules (id, task_id, workflow_id, target_kind, kind, config, enabled,
                                    next_run_at, created_at, updated_at)
             VALUES (?1, ?2, NULL, 'task', ?3, ?4, ?5, ?6, ?7, ?7)",
            rusqlite::params![id, task_id, kind, config_str, enabled, next_run_at, now],
        )
        .map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT id, task_id, workflow_id, target_kind, kind, config, enabled,
                    next_run_at, last_run_at, created_at, updated_at
             FROM schedules WHERE id = ?1",
            rusqlite::params![id],
            parse_schedule_row,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn update_schedule_record(
    db: &DbPool,
    schedule_id: &str,
    config: &Value,
    enabled: bool,
    next_run_at: Option<String>,
) -> Result<Schedule, String> {
    let pool = db.0.clone();
    let schedule_id = schedule_id.to_string();
    let config = config.clone();
    tokio::task::spawn_blocking(move || -> Result<Schedule, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        let config_str = serde_json::to_string(&config).map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE schedules
             SET config = ?1, enabled = ?2, next_run_at = ?3, updated_at = ?4
             WHERE id = ?5",
            rusqlite::params![config_str, enabled, next_run_at, now, schedule_id],
        )
        .map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT id, task_id, workflow_id, target_kind, kind, config, enabled,
                    next_run_at, last_run_at, created_at, updated_at
             FROM schedules WHERE id = ?1",
            rusqlite::params![schedule_id],
            parse_schedule_row,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn delete_schedule_record(db: &DbPool, schedule_id: &str) -> Result<(), String> {
    let pool = db.0.clone();
    let schedule_id = schedule_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM schedules WHERE id = ?1",
            rusqlite::params![schedule_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn parse_task_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let config_str: String = row.get(4)?;
    let tags_str: String = row.get(9)?;
    Ok(Task {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        kind: row.get(3)?,
        config: serde_json::from_str(&config_str).unwrap_or(Value::Null),
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
}

fn parse_schedule_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Schedule> {
    let config_str: String = row.get(5)?;
    Ok(Schedule {
        id: row.get(0)?,
        task_id: row.get(1)?,
        workflow_id: row.get(2)?,
        target_kind: row.get(3)?,
        kind: row.get(4)?,
        config: serde_json::from_str(&config_str).unwrap_or(Value::Null),
        enabled: row.get::<_, bool>(6)?,
        next_run_at: row.get(7)?,
        last_run_at: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn parse_chat_session_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChatSession> {
    Ok(ChatSession {
        id: row.get(0)?,
        agent_id: row.get(1)?,
        title: row.get(2)?,
        archived: row.get::<_, bool>(3)?,
        session_type: row.get(4)?,
        parent_session_id: row.get(5)?,
        source_bus_message_id: row.get(6)?,
        chain_depth: row.get(7)?,
        execution_state: row.get(8)?,
        finish_summary: row.get(9)?,
        terminal_error: row.get(10)?,
        source_agent_id: None,
        source_agent_name: None,
        source_session_id: None,
        source_session_title: None,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        project_id: row.get(13)?,
        worktree_name: row.get(14)?,
        worktree_branch: row.get(15)?,
        worktree_path: row.get(16)?,
    })
}

fn sync_schedule_upsert(ctx: &ToolExecutionContext, schedule: &Schedule) {
    let Some(client) = ctx.cloud_client.clone() else {
        return;
    };
    let schedule = schedule.clone();
    tokio::spawn(async move {
        if let Err(e) = client.upsert_schedule(&schedule).await {
            tracing::warn!("cloud upsert schedule: {}", e);
        }
    });
}

fn sync_schedule_delete(ctx: &ToolExecutionContext, schedule_id: &str) {
    let Some(client) = ctx.cloud_client.clone() else {
        return;
    };
    let schedule_id = schedule_id.to_string();
    tokio::spawn(async move {
        if let Err(e) = client.delete_by_id("schedules", &schedule_id).await {
            tracing::warn!("cloud delete schedule: {}", e);
        }
    });
}

fn sync_task_upsert(ctx: &ToolExecutionContext, task: &Task) {
    let Some(client) = ctx.cloud_client.clone() else {
        return;
    };
    let task = task.clone();
    tokio::spawn(async move {
        if let Err(e) = client.upsert_task(&task).await {
            tracing::warn!("cloud upsert pulse task: {}", e);
        }
    });
}

fn sync_chat_session_upsert(ctx: &ToolExecutionContext, session: &ChatSession) {
    let Some(client) = ctx.cloud_client.clone() else {
        return;
    };
    let session = session.clone();
    tokio::spawn(async move {
        if let Err(e) = client.upsert_chat_session(&session).await {
            tracing::warn!("cloud upsert pulse session: {}", e);
        }
    });
}
