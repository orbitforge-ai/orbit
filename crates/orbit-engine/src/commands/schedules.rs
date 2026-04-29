use crate::app_context::AppContext;
use crate::models::schedule::{CreateSchedule, RecurringConfig, Schedule};
use crate::scheduler::converter::next_n_runs;

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
pub async fn list_schedules(app: tauri::State<'_, AppContext>) -> Result<Vec<Schedule>, String> {
    app.repos.schedules().list().await
}

#[tauri::command]
pub async fn get_schedules_for_task(
    task_id: String,
    app: tauri::State<'_, AppContext>,
) -> Result<Vec<Schedule>, String> {
    app.repos.schedules().list_for_task(&task_id).await
}

#[tauri::command]
pub async fn get_schedules_for_workflow(
    workflow_id: String,
    app: tauri::State<'_, AppContext>,
) -> Result<Vec<Schedule>, String> {
    app.repos.schedules().list_for_workflow(&workflow_id).await
}

#[tauri::command]
pub async fn create_schedule(
    payload: CreateSchedule,
    app: tauri::State<'_, AppContext>,
) -> Result<Schedule, String> {
    let cloud = app.cloud.clone();
    let sched = app.repos.schedules().create(payload).await?;
    cloud_upsert_schedule!(cloud, sched);
    Ok(sched)
}

#[tauri::command]
pub async fn toggle_schedule(
    id: String,
    enabled: bool,
    app: tauri::State<'_, AppContext>,
) -> Result<(), String> {
    let cloud = app.cloud.clone();
    let sched = app.repos.schedules().toggle(&id, enabled).await?;
    cloud_upsert_schedule!(cloud, sched);
    Ok(())
}

#[tauri::command]
pub async fn delete_schedule(
    id: String,
    app: tauri::State<'_, AppContext>,
) -> Result<(), String> {
    let cloud = app.cloud.clone();
    app.repos.schedules().delete(&id).await?;
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

mod http {
    use super::*;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct TaskIdArgs {
        task_id: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct WorkflowIdArgs {
        workflow_id: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CreateArgs {
        payload: CreateSchedule,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ToggleArgs {
        id: String,
        enabled: bool,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct IdArgs {
        id: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct PreviewArgs {
        config: serde_json::Value,
        n: usize,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("list_schedules", |ctx, _args| async move {
            let r = ctx.repos.schedules().list().await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("get_schedules_for_task", |ctx, args| async move {
            let a: TaskIdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx.repos.schedules().list_for_task(&a.task_id).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("get_schedules_for_workflow", |ctx, args| async move {
            let a: WorkflowIdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx
                .repos
                .schedules()
                .list_for_workflow(&a.workflow_id)
                .await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("create_schedule", |ctx, args| async move {
            let a: CreateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            let sched = ctx.repos.schedules().create(a.payload).await?;
            cloud_upsert_schedule!(cloud, sched);
            serde_json::to_value(sched).map_err(|e| e.to_string())
        });
        reg.register("toggle_schedule", |ctx, args| async move {
            let a: ToggleArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            let sched = ctx.repos.schedules().toggle(&a.id, a.enabled).await?;
            cloud_upsert_schedule!(cloud, sched);
            Ok(serde_json::Value::Null)
        });
        reg.register("delete_schedule", |ctx, args| async move {
            let a: IdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            ctx.repos.schedules().delete(&a.id).await?;
            if let Some(client) = cloud.get() {
                let id = a.id.clone();
                tokio::spawn(async move {
                    if let Err(e) = client.delete_by_id("schedules", &id).await {
                        tracing::warn!("cloud delete schedules: {}", e);
                    }
                });
            }
            Ok(serde_json::Value::Null)
        });
        reg.register("preview_next_runs", |_ctx, args| async move {
            let a: PreviewArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = preview_next_runs(a.config, a.n)?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
    }
}

pub use http::register as register_http;
