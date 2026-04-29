use chrono::Utc;
use serde_json::Value;

use crate::db::DbPool;
use crate::models::workflow_run::{WorkflowRun, WorkflowRunStep, WorkflowRunSummary};

pub(crate) const STATUS_QUEUED: &str = "queued";
pub(crate) const STATUS_RUNNING: &str = "running";
pub(crate) const STATUS_SUCCESS: &str = "success";
pub(crate) const STATUS_FAILED: &str = "failed";
pub(crate) const STATUS_SKIPPED: &str = "skipped";

pub fn load_run_with_steps(
    pool: &DbPool,
    run_id: &str,
) -> Result<(WorkflowRun, Vec<WorkflowRunStep>), String> {
    load_run_with_steps_for_tenant(pool, run_id, "local")
}

pub fn load_run_with_steps_for_tenant(
    pool: &DbPool,
    run_id: &str,
    tenant_id: &str,
) -> Result<(WorkflowRun, Vec<WorkflowRunStep>), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let run = conn
        .query_row(
            "SELECT id, workflow_id, workflow_version, graph_snapshot, trigger_kind,
                    trigger_data, status, error, started_at, completed_at, created_at
             FROM workflow_runs WHERE id = ?1 AND tenant_id = ?2",
            rusqlite::params![run_id, tenant_id],
            map_run_row,
        )
        .map_err(|e| format!("workflow run {} not found: {}", run_id, e))?;

    let mut stmt = conn
        .prepare(
            "SELECT id, run_id, node_id, node_type, status, input, output, error,
                    started_at, completed_at, sequence
             FROM workflow_run_steps WHERE run_id = ?1 AND tenant_id = ?2 ORDER BY sequence ASC",
        )
        .map_err(|e| e.to_string())?;
    let steps = stmt
        .query_map(rusqlite::params![run_id, tenant_id], map_step_row)
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
    list_runs_for_workflow_for_tenant(pool, workflow_id, limit, "local")
}

pub fn list_runs_for_workflow_for_tenant(
    pool: &DbPool,
    workflow_id: &str,
    limit: i64,
    tenant_id: &str,
) -> Result<Vec<WorkflowRun>, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, workflow_id, workflow_version, graph_snapshot, trigger_kind,
                    trigger_data, status, error, started_at, completed_at, created_at
             FROM workflow_runs WHERE workflow_id = ?1 AND tenant_id = ?2
             ORDER BY created_at DESC LIMIT ?3",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            rusqlite::params![workflow_id, tenant_id, limit],
            map_run_row,
        )
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

#[allow(dead_code)]
pub fn list_runs_for_project(
    pool: &DbPool,
    project_id: &str,
    limit: i64,
) -> Result<Vec<WorkflowRunSummary>, String> {
    list_runs_for_project_for_tenant(pool, project_id, limit, "local")
}

pub fn list_runs_for_project_for_tenant(
    pool: &DbPool,
    project_id: &str,
    limit: i64,
    tenant_id: &str,
) -> Result<Vec<WorkflowRunSummary>, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT wr.id, wr.workflow_id, pw.name, wr.workflow_version, wr.trigger_kind,
                    wr.status, wr.error, wr.started_at, wr.completed_at, wr.created_at
             FROM workflow_runs wr
             INNER JOIN project_workflows pw ON pw.id = wr.workflow_id AND pw.tenant_id = wr.tenant_id
             WHERE pw.project_id = ?1 AND wr.tenant_id = ?2
             ORDER BY wr.created_at DESC LIMIT ?3",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            rusqlite::params![project_id, tenant_id, limit],
            map_run_summary_row,
        )
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn cancel_run(pool: &DbPool, run_id: &str) -> Result<(), String> {
    cancel_run_for_tenant(pool, run_id, "local")
}

pub fn cancel_run_for_tenant(pool: &DbPool, run_id: &str, tenant_id: &str) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE workflow_runs
         SET status = ?1, error = COALESCE(error, 'cancelled'), completed_at = ?2
         WHERE id = ?3 AND tenant_id = ?4 AND status IN ('queued', 'running')",
        rusqlite::params![STATUS_FAILED, now, run_id, tenant_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE workflow_run_steps
         SET status = ?1, error = COALESCE(error, 'cancelled'), completed_at = ?2
         WHERE run_id = ?3 AND tenant_id = ?4 AND status IN ('queued', 'running')",
        rusqlite::params![STATUS_SKIPPED, now, run_id, tenant_id],
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

fn map_run_summary_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkflowRunSummary> {
    Ok(WorkflowRunSummary {
        id: row.get(0)?,
        workflow_id: row.get(1)?,
        workflow_name: row.get(2)?,
        workflow_version: row.get(3)?,
        trigger_kind: row.get(4)?,
        status: row.get(5)?,
        error: row.get(6)?,
        started_at: row.get(7)?,
        completed_at: row.get(8)?,
        created_at: row.get(9)?,
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
