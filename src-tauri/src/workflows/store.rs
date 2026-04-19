use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use tauri::Emitter;
use ulid::Ulid;

use crate::db::DbPool;
use crate::models::project_workflow::{ProjectWorkflow, WorkflowGraph};
use crate::models::workflow_run::{WorkflowRun, WorkflowRunStep};

pub(crate) const STATUS_QUEUED: &str = "queued";
pub(crate) const STATUS_RUNNING: &str = "running";
pub(crate) const STATUS_SUCCESS: &str = "success";
pub(crate) const STATUS_FAILED: &str = "failed";
pub(crate) const STATUS_SKIPPED: &str = "skipped";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowRunEventPayload {
    workflow_id: String,
    run_id: String,
    status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowRunStepEventPayload {
    workflow_id: String,
    run_id: String,
    step_id: String,
    node_id: String,
    node_type: String,
    status: String,
}

pub(crate) async fn load_workflow(
    db: &DbPool,
    workflow_id: &str,
) -> Result<ProjectWorkflow, String> {
    let pool = db.0.clone();
    let id = workflow_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<ProjectWorkflow, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT id, project_id, name, description, enabled, graph, trigger_kind,
                    trigger_config, version, created_at, updated_at
             FROM project_workflows WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                let graph_str: String = row.get(5)?;
                let trigger_cfg_str: String = row.get(7)?;
                let graph: WorkflowGraph = serde_json::from_str(&graph_str).unwrap_or_default();
                let trigger_config: Value =
                    serde_json::from_str(&trigger_cfg_str).unwrap_or(Value::Null);
                Ok(ProjectWorkflow {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    name: row.get(2)?,
                    description: row.get(3)?,
                    enabled: row.get::<_, bool>(4)?,
                    graph,
                    trigger_kind: row.get(6)?,
                    trigger_config,
                    version: row.get(8)?,
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                })
            },
        )
        .map_err(|e| format!("workflow {} not found: {}", id, e))
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(crate) async fn insert_run(
    db: &DbPool,
    app: &tauri::AppHandle,
    workflow: &ProjectWorkflow,
    trigger_kind: &str,
    trigger_data: &Value,
) -> Result<WorkflowRun, String> {
    let pool = db.0.clone();
    let id = Ulid::new().to_string();
    let now = Utc::now().to_rfc3339();
    let workflow_id = workflow.id.clone();
    let workflow_version = workflow.version;
    let graph_str = serde_json::to_string(&workflow.graph).unwrap_or_else(|_| "{}".into());
    let trigger_kind = trigger_kind.to_string();
    let trigger_data_str = serde_json::to_string(trigger_data).unwrap_or_else(|_| "{}".into());

    let id_clone = id.clone();
    let now_clone = now.clone();
    let trigger_kind_clone = trigger_kind.clone();
    let trigger_data_str_clone = trigger_data_str.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO workflow_runs (id, workflow_id, workflow_version, graph_snapshot,
                                        trigger_kind, trigger_data, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                id_clone,
                workflow_id,
                workflow_version,
                graph_str,
                trigger_kind_clone,
                trigger_data_str_clone,
                STATUS_QUEUED,
                now_clone,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;

    let run = WorkflowRun {
        id,
        workflow_id: workflow.id.clone(),
        workflow_version,
        graph_snapshot: serde_json::to_value(&workflow.graph).unwrap_or(Value::Null),
        trigger_kind,
        trigger_data: trigger_data.clone(),
        status: STATUS_QUEUED.to_string(),
        error: None,
        started_at: None,
        completed_at: None,
        created_at: now,
    };
    let _ = app.emit(
        "workflow_run:created",
        WorkflowRunEventPayload {
            workflow_id: run.workflow_id.clone(),
            run_id: run.id.clone(),
            status: run.status.clone(),
        },
    );
    Ok(run)
}

pub(crate) async fn update_run_status(
    db: &DbPool,
    app: &tauri::AppHandle,
    workflow_id: &str,
    run_id: &str,
    status: &str,
    error: Option<&str>,
    started_at: Option<&str>,
    completed_at: Option<&str>,
) -> Result<(), String> {
    let pool = db.0.clone();
    let workflow_id = workflow_id.to_string();
    let run_id = run_id.to_string();
    let status = status.to_string();
    let error = error.map(String::from);
    let started_at = started_at.map(String::from);
    let completed_at = completed_at.map(String::from);
    let app = app.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE workflow_runs SET status = ?1, error = COALESCE(?2, error),
                                      started_at = COALESCE(?3, started_at),
                                      completed_at = COALESCE(?4, completed_at)
             WHERE id = ?5",
            rusqlite::params![status, error, started_at, completed_at, run_id],
        )
        .map_err(|e| e.to_string())?;
        let _ = app.emit(
            "workflow_run:updated",
            WorkflowRunEventPayload {
                workflow_id,
                run_id,
                status,
            },
        );
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(crate) async fn fail_run(
    db: &DbPool,
    app: &tauri::AppHandle,
    workflow_id: &str,
    run_id: &str,
    err: &str,
) -> Result<(), String> {
    let now = Utc::now().to_rfc3339();
    update_run_status(
        db,
        app,
        workflow_id,
        run_id,
        STATUS_FAILED,
        Some(err),
        None,
        Some(&now),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert_step(
    db: &DbPool,
    app: &tauri::AppHandle,
    workflow_id: &str,
    step_id: &str,
    run_id: &str,
    node_id: &str,
    node_type: &str,
    status: &str,
    input: &Value,
    started_at: Option<&str>,
    sequence: i64,
) -> Result<(), String> {
    let pool = db.0.clone();
    let workflow_id = workflow_id.to_string();
    let step_id = step_id.to_string();
    let run_id = run_id.to_string();
    let node_id = node_id.to_string();
    let node_type = node_type.to_string();
    let status = status.to_string();
    let input_str = serde_json::to_string(input).unwrap_or_else(|_| "{}".into());
    let started_at = started_at.map(String::from);
    let app = app.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO workflow_run_steps (id, run_id, node_id, node_type, status, input,
                                              started_at, sequence)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                step_id, run_id, node_id, node_type, status, input_str, started_at, sequence,
            ],
        )
        .map_err(|e| e.to_string())?;
        let _ = app.emit(
            "workflow_run:step",
            WorkflowRunStepEventPayload {
                workflow_id,
                run_id,
                step_id,
                node_id,
                node_type,
                status,
            },
        );
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn finish_step(
    db: &DbPool,
    app: &tauri::AppHandle,
    workflow_id: &str,
    run_id: &str,
    step_id: &str,
    node_id: &str,
    node_type: &str,
    status: &str,
    output: Option<&Value>,
    error: Option<&str>,
    completed_at: &str,
) -> Result<(), String> {
    let pool = db.0.clone();
    let workflow_id = workflow_id.to_string();
    let run_id = run_id.to_string();
    let step_id = step_id.to_string();
    let node_id = node_id.to_string();
    let node_type = node_type.to_string();
    let status = status.to_string();
    let output_str = output.map(|v| serde_json::to_string(v).unwrap_or_else(|_| "{}".into()));
    let error = error.map(String::from);
    let completed_at = completed_at.to_string();
    let app = app.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE workflow_run_steps SET status = ?1, output = ?2, error = ?3,
                                            completed_at = ?4
             WHERE id = ?5",
            rusqlite::params![status, output_str, error, completed_at, step_id],
        )
        .map_err(|e| e.to_string())?;
        let _ = app.emit(
            "workflow_run:step",
            WorkflowRunStepEventPayload {
                workflow_id,
                run_id,
                step_id,
                node_id,
                node_type,
                status,
            },
        );
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub fn load_run_with_steps(
    pool: &DbPool,
    run_id: &str,
) -> Result<(WorkflowRun, Vec<WorkflowRunStep>), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let run = conn
        .query_row(
            "SELECT id, workflow_id, workflow_version, graph_snapshot, trigger_kind,
                    trigger_data, status, error, started_at, completed_at, created_at
             FROM workflow_runs WHERE id = ?1",
            rusqlite::params![run_id],
            map_run_row,
        )
        .map_err(|e| format!("workflow run {} not found: {}", run_id, e))?;

    let mut stmt = conn
        .prepare(
            "SELECT id, run_id, node_id, node_type, status, input, output, error,
                    started_at, completed_at, sequence
             FROM workflow_run_steps WHERE run_id = ?1 ORDER BY sequence ASC",
        )
        .map_err(|e| e.to_string())?;
    let steps = stmt
        .query_map(rusqlite::params![run_id], map_step_row)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok((run, steps))
}

pub fn list_runs_for_workflow(
    pool: &DbPool,
    workflow_id: &str,
    limit: i64,
) -> Result<Vec<WorkflowRun>, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, workflow_id, workflow_version, graph_snapshot, trigger_kind,
                    trigger_data, status, error, started_at, completed_at, created_at
             FROM workflow_runs WHERE workflow_id = ?1
             ORDER BY created_at DESC LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![workflow_id, limit], map_run_row)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn cancel_run(pool: &DbPool, run_id: &str) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE workflow_runs
         SET status = ?1, error = COALESCE(error, 'cancelled'), completed_at = ?2
         WHERE id = ?3 AND status IN ('queued', 'running')",
        rusqlite::params![STATUS_FAILED, now, run_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE workflow_run_steps
         SET status = ?1, error = COALESCE(error, 'cancelled'), completed_at = ?2
         WHERE run_id = ?3 AND status IN ('queued', 'running')",
        rusqlite::params![STATUS_SKIPPED, now, run_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn map_run_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkflowRun> {
    let graph_str: String = row.get(3)?;
    let trigger_str: String = row.get(5)?;
    Ok(WorkflowRun {
        id: row.get(0)?,
        workflow_id: row.get(1)?,
        workflow_version: row.get(2)?,
        graph_snapshot: serde_json::from_str(&graph_str).unwrap_or(Value::Null),
        trigger_kind: row.get(4)?,
        trigger_data: serde_json::from_str(&trigger_str).unwrap_or(Value::Null),
        status: row.get(6)?,
        error: row.get(7)?,
        started_at: row.get(8)?,
        completed_at: row.get(9)?,
        created_at: row.get(10)?,
    })
}

fn map_step_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkflowRunStep> {
    let input_str: String = row.get(5)?;
    let output_opt: Option<String> = row.get(6)?;
    Ok(WorkflowRunStep {
        id: row.get(0)?,
        run_id: row.get(1)?,
        node_id: row.get(2)?,
        node_type: row.get(3)?,
        status: row.get(4)?,
        input: serde_json::from_str(&input_str).unwrap_or(Value::Null),
        output: output_opt.and_then(|s| serde_json::from_str(&s).ok()),
        error: row.get(7)?,
        started_at: row.get(8)?,
        completed_at: row.get(9)?,
        sequence: row.get(10)?,
    })
}
