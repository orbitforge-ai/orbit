use ulid::Ulid;

use crate::db::DbPool;
use crate::executor::engine::{ExecutorTx, RunRequest};
use crate::executor::keychain;
use crate::models::task::Task;

#[tauri::command]
pub async fn set_api_key(provider: String, key: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || keychain::store_api_key(&provider, &key))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn has_api_key(provider: String) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || Ok(keychain::has_api_key(&provider)))
        .await
        .map_err(|e: tokio::task::JoinError| e.to_string())?
}

#[tauri::command]
pub async fn delete_api_key(provider: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || keychain::delete_api_key(&provider))
        .await
        .map_err(|e| e.to_string())?
}

/// Trigger an autonomous agent loop run.
/// Creates an ephemeral agent_loop task and dispatches it to the executor.
#[tauri::command]
pub async fn trigger_agent_loop(
    agent_id: String,
    goal: String,
    db: tauri::State<'_, DbPool>,
    executor_tx: tauri::State<'_, ExecutorTx>,
) -> Result<String, String> {
    let pool = db.0.clone();
    let tx = executor_tx.0.clone();

    let (task, run_id, _log_path) =
        tokio::task::spawn_blocking(move || -> Result<(Task, String, String), String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            let now = chrono::Utc::now().to_rfc3339();
            let task_id = Ulid::new().to_string();
            let run_id = Ulid::new().to_string();

            let config = serde_json::json!({
                "goal": goal,
            });

            // Create ephemeral task
            conn.execute(
                "INSERT INTO tasks (id, name, description, kind, config, max_duration_seconds, max_retries, retry_delay_seconds, concurrency_policy, tags, agent_id, enabled, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'agent_loop', ?4, 7200, 0, 60, 'allow', '[]', ?5, 1, ?6, ?6)",
                rusqlite::params![
                    task_id,
                    format!("Agent loop: {}", &goal.chars().take(60).collect::<String>()),
                    Some(&goal),
                    config.to_string(),
                    agent_id,
                    now,
                ],
            )
            .map_err(|e| e.to_string())?;

            let log_path = format!(
                "{}/.orbit/logs/{}.log",
                std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
                run_id
            );

            // Create run record
            conn.execute(
                "INSERT INTO runs (id, task_id, agent_id, state, trigger, log_path, retry_count, metadata, created_at)
                 VALUES (?1, ?2, ?3, 'pending', 'manual', ?4, 0, '{}', ?5)",
                rusqlite::params![run_id, task_id, agent_id, log_path, now],
            )
            .map_err(|e| e.to_string())?;

            // Load task back for the RunRequest
            let task = conn
                .query_row(
                    "SELECT id, name, description, kind, config, max_duration_seconds, max_retries, retry_delay_seconds, concurrency_policy, tags, agent_id, session_id, enabled, created_at, updated_at
                     FROM tasks WHERE id = ?1",
                    rusqlite::params![task_id],
                    |row| {
                        let tags_str: String = row.get(9)?;
                        let config_str: String = row.get(4)?;
                        Ok(Task {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            description: row.get(2)?,
                            kind: row.get(3)?,
                            config: serde_json::from_str(&config_str).unwrap_or_default(),
                            max_duration_seconds: row.get(5)?,
                            max_retries: row.get(6)?,
                            retry_delay_seconds: row.get(7)?,
                            concurrency_policy: row.get(8)?,
                            tags: serde_json::from_str(&tags_str).unwrap_or_default(),
                            agent_id: row.get(10)?,
                            session_id: row.get(11)?,
                            enabled: row.get(12)?,
                            created_at: row.get(13)?,
                            updated_at: row.get(14)?,
                        })
                    },
                )
                .map_err(|e| e.to_string())?;

            Ok((task, run_id, log_path))
        })
        .await
        .map_err(|e| e.to_string())??;

    let run_id_clone = run_id.clone();
    tx.send(RunRequest {
        run_id: run_id_clone,
        task,
        schedule_id: None,
        trigger: "manual".to_string(),
        retry_count: 0,
        parent_run_id: None,
    })
    .map_err(|e| format!("failed to send to executor: {}", e))?;

    Ok(run_id)
}
