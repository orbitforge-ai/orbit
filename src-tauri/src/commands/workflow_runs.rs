use serde_json::Value;

use crate::db::DbPool;
use crate::models::workflow_run::{WorkflowRun, WorkflowRunSummary, WorkflowRunWithSteps};
use crate::workflows::orchestrator::{cancel_run, list_runs_for_workflow, load_run_with_steps};
use crate::workflows::store::list_runs_for_project;
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
pub async fn list_project_workflow_runs(
    project_id: String,
    limit: Option<i64>,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<WorkflowRunSummary>, String> {
    let pool = db.inner().clone();
    let limit = limit.unwrap_or(50).clamp(1, 200);
    tokio::task::spawn_blocking(move || list_runs_for_project(&pool, &project_id, limit))
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

mod http {
    use tauri::Manager;
    use super::*;
    use crate::db::DbPool;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct StartArgs { workflow_id: String, #[serde(default)] trigger_data: Option<Value> }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ListArgs { workflow_id: String, #[serde(default)] limit: Option<i64> }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ListProjectArgs { project_id: String, #[serde(default)] limit: Option<i64> }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RunIdArgs { run_id: String }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("start_workflow_run", |ctx, args| async move {
            let app = ctx.app()?;
            let a: StartArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = start_workflow_run(a.workflow_id, a.trigger_data, app.state::<DbPool>(), app.clone()).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("list_workflow_runs", |ctx, args| async move {
            let app = ctx.app()?;
            let a: ListArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = list_workflow_runs(a.workflow_id, a.limit, app.state::<DbPool>()).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("list_project_workflow_runs", |ctx, args| async move {
            let app = ctx.app()?;
            let a: ListProjectArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = list_project_workflow_runs(a.project_id, a.limit, app.state::<DbPool>()).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("get_workflow_run", |ctx, args| async move {
            let app = ctx.app()?;
            let a: RunIdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = get_workflow_run(a.run_id, app.state::<DbPool>()).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("cancel_workflow_run", |ctx, args| async move {
            let app = ctx.app()?;
            let a: RunIdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            cancel_workflow_run(a.run_id, app.state::<DbPool>()).await?;
            Ok(serde_json::Value::Null)
        });
    }
}

pub use http::register as register_http;
