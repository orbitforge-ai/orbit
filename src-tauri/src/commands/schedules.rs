use ulid::Ulid;

use crate::db::cloud::CloudClientState;
use crate::db::DbPool;
use crate::models::schedule::{CreateSchedule, RecurringConfig, Schedule};
use crate::scheduler::converter::{next_n_runs, to_cron};

const SCHEDULE_COLUMNS: &str = "id, task_id, workflow_id, target_kind, kind, config, enabled, \
                                next_run_at, last_run_at, created_at, updated_at";

fn map_schedule_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Schedule> {
    let config_str: String = row.get(5)?;
    Ok(Schedule {
        id: row.get(0)?,
        task_id: row.get(1)?,
        workflow_id: row.get(2)?,
        target_kind: row.get(3)?,
        kind: row.get(4)?,
        config: serde_json::from_str(&config_str).unwrap_or(serde_json::Value::Null),
        enabled: row.get::<_, bool>(6)?,
        next_run_at: row.get(7)?,
        last_run_at: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

macro_rules! cloud_upsert_schedule {
    ($cloud:expr, $sched:expr) => {
        if let Some(client) = $cloud.get() {
            let s = $sched.clone();
            tokio::spawn(async move {
                if let Err(e) = client.upsert_schedule(&s).await {
                    tracing::warn!("cloud upsert schedule: {}", e);
                }
            });
        }
    };
}

#[tauri::command]
pub async fn list_schedules(db: tauri::State<'_, DbPool>) -> Result<Vec<Schedule>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let sql = format!("SELECT {SCHEDULE_COLUMNS} FROM schedules ORDER BY created_at DESC");
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

        let schedules = stmt
            .query_map([], map_schedule_row)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        Ok(schedules)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_schedules_for_task(
    task_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<Schedule>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let sql = format!("SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE task_id = ?1");
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

        let schedules = stmt
            .query_map(rusqlite::params![task_id], map_schedule_row)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        Ok(schedules)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_schedules_for_workflow(
    workflow_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<Schedule>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let sql = format!("SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE workflow_id = ?1");
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

        let schedules = stmt
            .query_map(rusqlite::params![workflow_id], map_schedule_row)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        Ok(schedules)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn create_schedule(
    payload: CreateSchedule,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<Schedule, String> {
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let sched: Schedule = tokio::task::spawn_blocking(move || -> Result<Schedule, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let id = Ulid::new().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let config_str = serde_json::to_string(&payload.config).map_err(|e| e.to_string())?;

        let target_kind = payload
            .target_kind
            .clone()
            .unwrap_or_else(|| "task".to_string());
        if target_kind != "task" && target_kind != "workflow" {
            return Err(format!("invalid target_kind: {}", target_kind));
        }
        match target_kind.as_str() {
            "task" => {
                if payload.task_id.is_none() || payload.workflow_id.is_some() {
                    return Err("task schedule requires task_id and no workflow_id".into());
                }
            }
            "workflow" => {
                if payload.workflow_id.is_none() || payload.task_id.is_some() {
                    return Err("workflow schedule requires workflow_id and no task_id".into());
                }
            }
            _ => unreachable!(),
        }

        let next_run_at = if payload.kind == "recurring" {
            let cfg: RecurringConfig = serde_json::from_value(payload.config.clone())
                .map_err(|e| format!("invalid recurring config: {}", e))?;
            to_cron(&cfg)
                .ok()
                .and_then(|_| next_n_runs(&cfg, 1).into_iter().next())
        } else {
            None
        };

        conn.execute(
            "INSERT INTO schedules (id, task_id, workflow_id, target_kind, kind, config, enabled,
                                    next_run_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?8)",
            rusqlite::params![
                id,
                payload.task_id,
                payload.workflow_id,
                target_kind,
                payload.kind,
                config_str,
                next_run_at,
                now
            ],
        )
        .map_err(|e| e.to_string())?;

        let sql = format!("SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE id = ?1");
        conn.query_row(&sql, rusqlite::params![id], map_schedule_row)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_upsert_schedule!(cloud, sched);
    Ok(sched)
}

#[tauri::command]
pub async fn toggle_schedule(
    id: String,
    enabled: bool,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let id_clone = id.clone();
    let sched: Schedule = tokio::task::spawn_blocking(move || -> Result<Schedule, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE schedules SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![enabled as i64, now, id_clone],
        )
        .map_err(|e| e.to_string())?;
        let sql = format!("SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE id = ?1");
        conn.query_row(&sql, rusqlite::params![id_clone], map_schedule_row)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_upsert_schedule!(cloud, sched);
    Ok(())
}

#[tauri::command]
pub async fn delete_schedule(
    id: String,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let id_clone = id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM schedules WHERE id = ?1",
            rusqlite::params![id_clone],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;

    if let Some(client) = cloud.get() {
        let id = id.clone();
        tokio::spawn(async move {
            if let Err(e) = client.delete_by_id("schedules", &id).await {
                tracing::warn!("cloud delete schedules: {}", e);
            }
        });
    }
    Ok(())
}

/// Returns the next N run times for a recurring config — used by the UI preview.
#[tauri::command]
pub fn preview_next_runs(config: serde_json::Value, n: usize) -> Result<Vec<String>, String> {
    let cfg: RecurringConfig =
        serde_json::from_value(config).map_err(|e| format!("invalid config: {}", e))?;
    Ok(next_n_runs(&cfg, n))
}
