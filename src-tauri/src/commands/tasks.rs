use ulid::Ulid;

use crate::db::DbPool;
use crate::executor::engine::{ ExecutorTx, RunRequest };
use crate::models::task::{ CreateTask, Task, UpdateTask };

#[tauri::command]
pub async fn list_tasks(db: tauri::State<'_, DbPool>) -> Result<Vec<Task>, String> {
  let pool = db.0.clone();
  tokio::task
    ::spawn_blocking(move || {
      let conn = pool.get().map_err(|e| e.to_string())?;
      let mut stmt = conn
        .prepare(
          "SELECT id, name, description, kind, config, max_duration_seconds, max_retries,
                        retry_delay_seconds, concurrency_policy, tags, agent_id,
                        enabled, created_at, updated_at
                 FROM tasks ORDER BY created_at DESC"
        )
        .map_err(|e| e.to_string())?;

      let tasks = stmt
        .query_map([], |row| {
          let config_str: String = row.get(4)?;
          let tags_str: String = row.get(9)?;
          Ok(Task {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            kind: row.get(3)?,
            config: serde_json::from_str(&config_str).unwrap_or(serde_json::Value::Null),
            max_duration_seconds: row.get(5)?,
            max_retries: row.get(6)?,
            retry_delay_seconds: row.get(7)?,
            concurrency_policy: row.get(8)?,
            tags: serde_json::from_str(&tags_str).unwrap_or_default(),
            agent_id: row.get(10)?,
            enabled: row.get::<_, bool>(11)?,
            created_at: row.get(12)?,
            updated_at: row.get(13)?,
          })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

      Ok(tasks)
    }).await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_task(id: String, db: tauri::State<'_, DbPool>) -> Result<Task, String> {
  let pool = db.0.clone();
  tokio::task
    ::spawn_blocking(move || {
      let conn = pool.get().map_err(|e| e.to_string())?;
      conn
        .query_row(
          "SELECT id, name, description, kind, config, max_duration_seconds, max_retries,
                    retry_delay_seconds, concurrency_policy, tags, agent_id,
                    enabled, created_at, updated_at
             FROM tasks WHERE id = ?1",
          rusqlite::params![id],
          |row| {
            let config_str: String = row.get(4)?;
            let tags_str: String = row.get(9)?;
            Ok(Task {
              id: row.get(0)?,
              name: row.get(1)?,
              description: row.get(2)?,
              kind: row.get(3)?,
              config: serde_json::from_str(&config_str).unwrap_or(serde_json::Value::Null),
              max_duration_seconds: row.get(5)?,
              max_retries: row.get(6)?,
              retry_delay_seconds: row.get(7)?,
              concurrency_policy: row.get(8)?,
              tags: serde_json::from_str(&tags_str).unwrap_or_default(),
              agent_id: row.get(10)?,
              enabled: row.get::<_, bool>(11)?,
              created_at: row.get(12)?,
              updated_at: row.get(13)?,
            })
          }
        )
        .map_err(|e| e.to_string())
    }).await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn create_task(
  payload: CreateTask,
  db: tauri::State<'_, DbPool>
) -> Result<Task, String> {
  let pool = db.0.clone();
  tokio::task
    ::spawn_blocking(move || {
      let conn = pool.get().map_err(|e| e.to_string())?;
      let id = Ulid::new().to_string();
      let now = chrono::Utc::now().to_rfc3339();
      let config_str = serde_json::to_string(&payload.config).map_err(|e| e.to_string())?;
      let tags_str = serde_json
        ::to_string(&payload.tags.unwrap_or_default())
        .map_err(|e| e.to_string())?;
      let max_duration = payload.max_duration_seconds.unwrap_or(3600);
      let max_retries = payload.max_retries.unwrap_or(0);
      let retry_delay = payload.retry_delay_seconds.unwrap_or(60);
      let concurrency = payload.concurrency_policy.unwrap_or_else(|| "allow".to_string());
      let agent_id = payload.agent_id.unwrap_or_else(|| "default".to_string());

      conn
        .execute(
          "INSERT INTO tasks (id, name, description, kind, config, max_duration_seconds,
                                max_retries, retry_delay_seconds, concurrency_policy, tags,
                                agent_id, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, ?12, ?12)",
          rusqlite::params![
            id,
            payload.name,
            payload.description,
            payload.kind,
            config_str,
            max_duration,
            max_retries,
            retry_delay,
            concurrency,
            tags_str,
            agent_id,
            now
          ]
        )
        .map_err(|e| e.to_string())?;

      conn
        .query_row(
          "SELECT id, name, description, kind, config, max_duration_seconds, max_retries,
                    retry_delay_seconds, concurrency_policy, tags, agent_id,
                    enabled, created_at, updated_at
             FROM tasks WHERE id = ?1",
          rusqlite::params![id],
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
            })
          }
        )
        .map_err(|e| e.to_string())
    }).await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn update_task(
  id: String,
  payload: UpdateTask,
  db: tauri::State<'_, DbPool>
) -> Result<Task, String> {
  let pool = db.0.clone();
  tokio::task
    ::spawn_blocking(move || {
      let conn = pool.get().map_err(|e| e.to_string())?;
      let now = chrono::Utc::now().to_rfc3339();

      if let Some(name) = &payload.name {
        conn
          .execute(
            "UPDATE tasks SET name = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![name, now, id]
          )
          .map_err(|e| e.to_string())?;
      }
      if let Some(desc) = &payload.description {
        conn
          .execute(
            "UPDATE tasks SET description = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![desc, now, id]
          )
          .map_err(|e| e.to_string())?;
      }
      if let Some(cfg) = &payload.config {
        let s = serde_json::to_string(cfg).map_err(|e| e.to_string())?;
        conn
          .execute(
            "UPDATE tasks SET config = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![s, now, id]
          )
          .map_err(|e| e.to_string())?;
      }
      if let Some(enabled) = payload.enabled {
        conn
          .execute(
            "UPDATE tasks SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![enabled as i64, now, id]
          )
          .map_err(|e| e.to_string())?;
      }
      if let Some(agent_id) = &payload.agent_id {
        conn
          .execute(
            "UPDATE tasks SET agent_id = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![agent_id, now, id]
          )
          .map_err(|e| e.to_string())?;
      }
      if let Some(max_duration) = payload.max_duration_seconds {
        conn
          .execute(
            "UPDATE tasks SET max_duration_seconds = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![max_duration, now, id]
          )
          .map_err(|e| e.to_string())?;
      }
      if let Some(max_retries) = payload.max_retries {
        conn
          .execute(
            "UPDATE tasks SET max_retries = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![max_retries, now, id]
          )
          .map_err(|e| e.to_string())?;
      }
      if let Some(retry_delay) = payload.retry_delay_seconds {
        conn
          .execute(
            "UPDATE tasks SET retry_delay_seconds = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![retry_delay, now, id]
          )
          .map_err(|e| e.to_string())?;
      }
      if let Some(policy) = &payload.concurrency_policy {
        conn
          .execute(
            "UPDATE tasks SET concurrency_policy = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![policy, now, id]
          )
          .map_err(|e| e.to_string())?;
      }
      if let Some(tags) = &payload.tags {
        let t = serde_json::to_string(tags).map_err(|e| e.to_string())?;
        conn
          .execute(
            "UPDATE tasks SET tags = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![t, now, id]
          )
          .map_err(|e| e.to_string())?;
      }

      conn
        .query_row(
          "SELECT id, name, description, kind, config, max_duration_seconds, max_retries,
                    retry_delay_seconds, concurrency_policy, tags, agent_id,
                    enabled, created_at, updated_at FROM tasks WHERE id = ?1",
          rusqlite::params![id],
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
            })
          }
        )
        .map_err(|e| e.to_string())
    }).await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_task(id: String, db: tauri::State<'_, DbPool>) -> Result<(), String> {
  let pool = db.0.clone();
  tokio::task
    ::spawn_blocking(move || {
      let conn = pool.get().map_err(|e| e.to_string())?;
      conn
        .execute("DELETE FROM tasks WHERE id = ?1", rusqlite::params![id])
        .map_err(|e| e.to_string())?;
      Ok(())
    }).await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn trigger_task(
  task_id: String,
  db: tauri::State<'_, DbPool>,
  executor_tx: tauri::State<'_, ExecutorTx>
) -> Result<String, String> {
  let pool = db.0.clone();
  let tx = executor_tx.0.clone();

  tokio::task
    ::spawn_blocking(move || {
      let conn = pool.get().map_err(|e| e.to_string())?;

      let task = conn
        .query_row(
          "SELECT id, name, description, kind, config, max_duration_seconds, max_retries,
                        retry_delay_seconds, concurrency_policy, tags, agent_id,
                        enabled, created_at, updated_at FROM tasks WHERE id = ?1 AND enabled = 1",
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
            })
          }
        )
        .map_err(|e| format!("task not found: {}", e))?;

      let run_id = Ulid::new().to_string();
      let now = chrono::Utc::now().to_rfc3339();
      let log_path = format!(
        "{}/.orbit/logs/{}.log",
        std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
        run_id
      );

      conn
        .execute(
          "INSERT INTO runs (id, task_id, schedule_id, agent_id, state, trigger, log_path, retry_count, metadata, created_at)
             VALUES (?1, ?2, NULL, ?3, 'pending', 'manual', ?4, 0, '{}', ?5)",
          rusqlite::params![run_id, task_id, task.agent_id, log_path, now]
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
