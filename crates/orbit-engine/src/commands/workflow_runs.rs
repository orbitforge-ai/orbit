//! Workflow run inspection commands.
//!
//! Read paths flow through `WorkflowRunRepo`. The `start` path keeps using
//! the orchestrator directly because it spawns the run loop. The `cancel`
//! path goes through the repo, which itself dispatches to the orchestrator
//! to flip state + nudge the loop.

use serde_json::Value;

use crate::app_context::AppContext;
use crate::models::workflow_run::{WorkflowRun, WorkflowRunSummary, WorkflowRunWithSteps};
use crate::workflows::WorkflowOrchestrator;

#[tauri::command]
pub async fn start_workflow_run(
    workflow_id: String,
    trigger_data: Option<Value>,
    app: tauri::State<'_, AppContext>,
) -> Result<WorkflowRun, String> {
    // Orchestrator-side: starting a run also boots the per-run state machine,
    // which the repo trait deliberately stays out of.
    let orchestrator = WorkflowOrchestrator::new(app.db.clone(), app.runtime.clone());
    orchestrator
        .start_run(workflow_id, "manual", trigger_data.unwrap_or(Value::Null))
        .await
}

#[tauri::command]
pub async fn list_workflow_runs(
    workflow_id: String,
    limit: Option<i64>,
    app: tauri::State<'_, AppContext>,
) -> Result<Vec<WorkflowRun>, String> {
    let limit = limit.unwrap_or(50);
    app.repos
        .workflow_runs()
        .list_for_workflow(&workflow_id, limit)
        .await
}

#[tauri::command]
pub async fn list_project_workflow_runs(
    project_id: String,
    limit: Option<i64>,
    app: tauri::State<'_, AppContext>,
) -> Result<Vec<WorkflowRunSummary>, String> {
    let limit = limit.unwrap_or(50);
    app.repos
        .workflow_runs()
        .list_for_project(&project_id, limit)
        .await
}

#[tauri::command]
pub async fn get_workflow_run(
    run_id: String,
    app: tauri::State<'_, AppContext>,
) -> Result<WorkflowRunWithSteps, String> {
    app.repos.workflow_runs().get_with_steps(&run_id).await
}

#[tauri::command]
pub async fn cancel_workflow_run(
    run_id: String,
    app: tauri::State<'_, AppContext>,
) -> Result<(), String> {
    app.repos.workflow_runs().cancel(&run_id).await
}

mod http {
    use super::*;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct StartArgs {
        workflow_id: String,
        #[serde(default)]
        trigger_data: Option<Value>,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ListArgs {
        workflow_id: String,
        #[serde(default)]
        limit: Option<i64>,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ListProjectArgs {
        project_id: String,
        #[serde(default)]
        limit: Option<i64>,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RunIdArgs {
        run_id: String,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("start_workflow_run", |ctx, args| async move {
            let a: StartArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = WorkflowOrchestrator::new(ctx.db.clone(), ctx.runtime.clone())
                .start_run(
                    a.workflow_id,
                    "manual",
                    a.trigger_data.unwrap_or(Value::Null),
                )
                .await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("list_workflow_runs", |ctx, args| async move {
            let a: ListArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let limit = a.limit.unwrap_or(50);
            let r = ctx
                .repos
                .workflow_runs()
                .list_for_workflow(&a.workflow_id, limit)
                .await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("list_project_workflow_runs", |ctx, args| async move {
            let a: ListProjectArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let limit = a.limit.unwrap_or(50);
            let r = ctx
                .repos
                .workflow_runs()
                .list_for_project(&a.project_id, limit)
                .await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("get_workflow_run", |ctx, args| async move {
            let a: RunIdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx.repos.workflow_runs().get_with_steps(&a.run_id).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("cancel_workflow_run", |ctx, args| async move {
            let a: RunIdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            ctx.repos.workflow_runs().cancel(&a.run_id).await?;
            Ok(serde_json::Value::Null)
        });
    }
}

pub use http::register as register_http;
