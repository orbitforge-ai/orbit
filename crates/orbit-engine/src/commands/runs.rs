//! Run inspection commands. All read-only — write paths (start/finish) live
//! in the executor since they're tied to process spawning. The on-disk run
//! log read still happens here because it's a filesystem op and not a DB op.

use crate::app_context::AppContext;
use crate::db::repos::RunListFilter;
use crate::models::run::{Run, RunSummary};

#[tauri::command]
pub async fn list_runs(
    limit: Option<i64>,
    offset: Option<i64>,
    task_id: Option<String>,
    state_filter: Option<String>,
    project_id: Option<String>,
    app: tauri::State<'_, AppContext>,
) -> Result<Vec<RunSummary>, String> {
    app.repos
        .runs()
        .list(RunListFilter {
            limit,
            offset,
            task_id,
            state_filter,
            project_id,
        })
        .await
}

#[tauri::command]
pub async fn get_run(id: String, app: tauri::State<'_, AppContext>) -> Result<Run, String> {
    app.repos
        .runs()
        .get(&id)
        .await?
        .ok_or_else(|| format!("run '{}' not found", id))
}

#[tauri::command]
pub async fn get_active_runs(app: tauri::State<'_, AppContext>) -> Result<Vec<RunSummary>, String> {
    app.repos.runs().list_active().await
}

#[tauri::command]
pub async fn get_agent_conversation(
    run_id: String,
    app: tauri::State<'_, AppContext>,
) -> Result<Option<serde_json::Value>, String> {
    app.repos.runs().agent_conversation(&run_id).await
}

#[tauri::command]
pub async fn list_sub_agent_runs(
    parent_run_id: String,
    app: tauri::State<'_, AppContext>,
) -> Result<Vec<RunSummary>, String> {
    app.repos.runs().list_sub_agents(&parent_run_id).await
}

#[tauri::command]
pub async fn read_run_log(
    run_id: String,
    app: tauri::State<'_, AppContext>,
) -> Result<String, String> {
    let log_path = app
        .repos
        .runs()
        .log_path(&run_id)
        .await?
        .ok_or_else(|| format!("run '{}' has no log path", run_id))?;
    tokio::fs::read_to_string(&log_path)
        .await
        .map_err(|e| format!("cannot read log file: {}", e))
}

mod http {
    use super::*;

    #[derive(serde::Deserialize, Default)]
    #[serde(default, rename_all = "camelCase")]
    struct ListRunsArgs {
        limit: Option<i64>,
        offset: Option<i64>,
        task_id: Option<String>,
        state_filter: Option<String>,
        project_id: Option<String>,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct IdArgs {
        id: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RunIdArgs {
        run_id: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ParentArgs {
        parent_run_id: String,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("list_runs", |ctx, args| async move {
            let a: ListRunsArgs = if args.is_null() {
                ListRunsArgs::default()
            } else {
                serde_json::from_value(args).map_err(|e| e.to_string())?
            };
            let r = ctx
                .repos
                .runs()
                .list(RunListFilter {
                    limit: a.limit,
                    offset: a.offset,
                    task_id: a.task_id,
                    state_filter: a.state_filter,
                    project_id: a.project_id,
                })
                .await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("get_run", |ctx, args| async move {
            let a: IdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx
                .repos
                .runs()
                .get(&a.id)
                .await?
                .ok_or_else(|| format!("run '{}' not found", a.id))?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("get_active_runs", |ctx, _args| async move {
            let r = ctx.repos.runs().list_active().await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("read_run_log", |ctx, args| async move {
            let a: RunIdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let log_path = ctx
                .repos
                .runs()
                .log_path(&a.run_id)
                .await?
                .ok_or_else(|| format!("run '{}' has no log path", a.run_id))?;
            let body = tokio::fs::read_to_string(&log_path)
                .await
                .map_err(|e| format!("cannot read log file: {}", e))?;
            Ok(serde_json::Value::String(body))
        });
        reg.register("get_agent_conversation", |ctx, args| async move {
            let a: RunIdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx.repos.runs().agent_conversation(&a.run_id).await?;
            Ok(r.unwrap_or(serde_json::Value::Null))
        });
        reg.register("list_sub_agent_runs", |ctx, args| async move {
            let a: ParentArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx.repos.runs().list_sub_agents(&a.parent_run_id).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
    }
}

pub use http::register as register_http;
