use serde_json::Value;

use crate::db::DbPool;
use crate::models::workflow_run::{WorkflowRun, WorkflowRunWithSteps};
use crate::workflows::orchestrator::{cancel_run, list_runs_for_workflow, load_run_with_steps};
use crate::workflows::WorkflowOrchestrator;

#[tauri::command]
pub async fn start_workflow_run(
    workflow_id: String,
    trigger_data: Option<Value>,
    db: tauri::State<'_, DbPool>,
    app: tauri::AppHandle,
) -> Result<WorkflowRun, String> {
    let orchestrator = WorkflowOrchestrator::new(db.inner().clone(), app);
    orchestrator
        .start_run(workflow_id, "manual", trigger_data.unwrap_or(Value::Null))
        .await
}

#[tauri::command]
pub async fn list_workflow_runs(
    workflow_id: String,
    limit: Option<i64>,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<WorkflowRun>, String> {
    let pool = db.inner().clone();
    let limit = limit.unwrap_or(50).clamp(1, 200);
    tokio::task::spawn_blocking(move || list_runs_for_workflow(&pool, &workflow_id, limit))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_workflow_run(
    run_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<WorkflowRunWithSteps, String> {
    let pool = db.inner().clone();
    tokio::task::spawn_blocking(move || -> Result<WorkflowRunWithSteps, String> {
        let (run, steps) = load_run_with_steps(&pool, &run_id)?;
        Ok(WorkflowRunWithSteps { run, steps })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn cancel_workflow_run(
    run_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<(), String> {
    let pool = db.inner().clone();
    tokio::task::spawn_blocking(move || cancel_run(&pool, &run_id))
        .await
        .map_err(|e| e.to_string())?
}
