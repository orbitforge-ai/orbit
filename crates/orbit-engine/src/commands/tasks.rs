use ulid::Ulid;

use crate::app_context::AppContext;
use crate::executor::engine::RunRequest;
use crate::executor::workspace;
use crate::models::task::{CreateTask, Task, UpdateTask};

macro_rules! cloud_upsert_task {
    ($cloud:expr, $task:expr) => {
        if let Some(client) = $cloud.get() {
            let t = $task.clone();
            tokio::spawn(async move {
                if let Err(e) = client.upsert_task(&t).await {
                    tracing::warn!("cloud upsert task: {}", e);
                }
            });
        }
    };
}

macro_rules! cloud_delete {
    ($cloud:expr, $table:expr, $id:expr) => {
        if let Some(client) = $cloud.get() {
            let id = $id.to_string();
            tokio::spawn(async move {
                if let Err(e) = client.delete_by_id($table, &id).await {
                    tracing::warn!("cloud delete {}: {}", $table, e);
                }
            });
        }
    };
}

/// Phase B.4: every list / get / create / update / delete now flows through
/// `AppContext::repos`. Both the Tauri command (via `tauri::State<AppContext>`)
/// and the shim adapter (via `ctx.repos`) hit the same `TaskRepo` trait, so
/// behavior is identical across desktop and headless server.

#[tauri::command]
pub async fn list_tasks(app: tauri::State<'_, AppContext>) -> Result<Vec<Task>, String> {
    app.repos.tasks().list().await
}

#[tauri::command]
pub async fn get_task(id: String, app: tauri::State<'_, AppContext>) -> Result<Task, String> {
    app.repos
        .tasks()
        .get(&id)
        .await?
        .ok_or_else(|| format!("task not found: {id}"))
}

#[tauri::command]
pub async fn create_task(
    payload: CreateTask,
    app: tauri::State<'_, AppContext>,
) -> Result<Task, String> {
    let cloud = app.cloud.clone();
    let task = app.repos.tasks().create(payload).await?;
    cloud_upsert_task!(cloud, task);
    Ok(task)
}

#[tauri::command]
pub async fn update_task(
    id: String,
    payload: UpdateTask,
    app: tauri::State<'_, AppContext>,
) -> Result<Task, String> {
    let cloud = app.cloud.clone();
    let task = app.repos.tasks().update(&id, payload).await?;
    cloud_upsert_task!(cloud, task);
    Ok(task)
}

#[tauri::command]
pub async fn delete_task(id: String, app: tauri::State<'_, AppContext>) -> Result<(), String> {
    let cloud = app.cloud.clone();
    app.repos.tasks().delete(&id).await?;
    cloud_delete!(cloud, "tasks", id);
    Ok(())
}

#[tauri::command]
pub async fn trigger_task(
    task_id: String,
    app: tauri::State<'_, AppContext>,
) -> Result<String, String> {
    trigger_task_inner(task_id, &app).await
}

async fn trigger_task_inner(task_id: String, app: &AppContext) -> Result<String, String> {
    let pool = app.db.0.clone();
    let tx = app.executor_tx.0.clone();

    tokio::task
    ::spawn_blocking(move || {
      let conn = pool.get().map_err(|e| e.to_string())?;

      let mut task = conn
        .query_row(
          "SELECT id, name, description, kind, config, max_duration_seconds, max_retries,
                        retry_delay_seconds, concurrency_policy, tags, agent_id,
                        enabled, created_at, updated_at, project_id FROM tasks WHERE id = ?1 AND enabled = 1",
          rusqlite::params![task_id],
          |row| {
            let cfg: String = row.get(4)?;
            let tags: String = row.get(9)?;
            Ok(Task {
              id: row.get(0)?,
              name: row.get(1)?,
              description: row.get(2)?,
              kind: row.get(3)?,
              config: serde_json::from_str(&cfg).unwrap_or(serde_json::Value::Null),
              max_duration_seconds: row.get(5)?,
              max_retries: row.get(6)?,
              retry_delay_seconds: row.get(7)?,
              concurrency_policy: row.get(8)?,
              tags: serde_json::from_str(&tags).unwrap_or_default(),
              agent_id: row.get(10)?,
              enabled: row.get::<_, bool>(11)?,
              created_at: row.get(12)?,
              updated_at: row.get(13)?,
              project_id: row.get(14)?,
            })
          }
        )
        .map_err(|e| format!("task not found: {}", e))?;

      // If the task belongs to a project, inject the project workspace as the default CWD
      // for shell_command and script_file tasks that don't already have a working directory.
      if let Some(ref project_id) = task.project_id.clone() {
        let project_cwd = workspace::project_workspace_dir(project_id)
          .to_string_lossy()
          .to_string();
        match task.kind.as_str() {
          "shell_command" | "script_file" => {
            if let Some(cfg_obj) = task.config.as_object_mut() {
              if !cfg_obj.contains_key("workingDirectory")
                || cfg_obj["workingDirectory"].is_null()
              {
                cfg_obj.insert(
                  "workingDirectory".to_string(),
                  serde_json::Value::String(project_cwd),
                );
              }
            }
          }
          _ => {}
        }
      }

      let run_id = Ulid::new().to_string();
      let now = chrono::Utc::now().to_rfc3339();
      let log_path = format!(
        "{}/.orbit/logs/{}.log",
        std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
        run_id
      );

      conn
        .execute(
          "INSERT INTO runs (id, task_id, schedule_id, agent_id, state, trigger, log_path, retry_count, metadata, project_id, created_at)
             VALUES (?1, ?2, NULL, ?3, 'pending', 'manual', ?4, 0, '{}', ?5, ?6)",
          rusqlite::params![run_id, task_id, task.agent_id, log_path, task.project_id, now]
        )
        .map_err(|e| e.to_string())?;

      tx
        .send(RunRequest {
          run_id: run_id.clone(),
          task,
          schedule_id: None,
          _trigger: "manual".to_string(),
          retry_count: 0,
          _parent_run_id: None,
          chain_depth: 0,
        })
        .map_err(|e| e.to_string())?;

      Ok(run_id)
    }).await
    .map_err(|e| e.to_string())?
}

mod http {
    use super::*;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct IdArgs {
        id: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CreateArgs {
        payload: CreateTask,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UpdateArgs {
        id: String,
        payload: UpdateTask,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct TriggerArgs {
        task_id: String,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        // CRUD goes through `ctx.repos.tasks()` — works in headless mode
        // (no Tauri runtime needed).
        reg.register("list_tasks", |ctx, _args| async move {
            let result = ctx.repos.tasks().list().await?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        });
        reg.register("get_task", |ctx, args| async move {
            let a: IdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx
                .repos
                .tasks()
                .get(&a.id)
                .await?
                .ok_or_else(|| format!("task not found: {}", a.id))?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("create_task", |ctx, args| async move {
            let a: CreateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            let r = ctx.repos.tasks().create(a.payload).await?;
            cloud_upsert_task!(cloud, r);
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("update_task", |ctx, args| async move {
            let a: UpdateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            let r = ctx.repos.tasks().update(&a.id, a.payload).await?;
            cloud_upsert_task!(cloud, r);
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("delete_task", |ctx, args| async move {
            let a: IdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            ctx.repos.tasks().delete(&a.id).await?;
            cloud_delete!(cloud, "tasks", a.id);
            Ok(serde_json::Value::Null)
        });
        // trigger_task inserts into `runs` and sends to the executor channel,
        // so it still goes through the coordinator path rather than TaskRepo.
        reg.register("trigger_task", |ctx, args| async move {
            let a: TriggerArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = trigger_task_inner(a.task_id, &ctx).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
    }
}

pub use http::register as register_http;
