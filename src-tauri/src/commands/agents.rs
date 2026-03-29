use crate::db::DbPool;
use crate::executor::workspace;
use crate::models::agent::{Agent, CreateAgent, UpdateAgent};

#[tauri::command]
pub async fn list_agents(db: tauri::State<'_, DbPool>) -> Result<Vec<Agent>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, name, description, state, max_concurrent_runs, heartbeat_at, created_at, updated_at
                 FROM agents ORDER BY created_at ASC",
            )
            .map_err(|e| e.to_string())?;

        let agents = stmt
            .query_map([], |row| {
                Ok(Agent {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    state: row.get(3)?,
                    max_concurrent_runs: row.get(4)?,
                    heartbeat_at: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        Ok(agents)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn create_agent(
    payload: CreateAgent,
    db: tauri::State<'_, DbPool>,
) -> Result<Agent, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let base_slug = workspace::slugify(&payload.name);
        let base_slug = if base_slug.is_empty() { "agent".to_string() } else { base_slug };

        // Ensure unique ID by appending a number suffix if needed
        let id = {
            let mut candidate = base_slug.clone();
            let mut suffix = 1;
            while conn.query_row(
                "SELECT 1 FROM agents WHERE id = ?1",
                rusqlite::params![candidate],
                |_| Ok(()),
            ).is_ok() {
                suffix += 1;
                candidate = format!("{}-{}", base_slug, suffix);
            }
            candidate
        };

        let now = chrono::Utc::now().to_rfc3339();
        let max_runs = payload.max_concurrent_runs.unwrap_or(5);

        conn.execute(
            "INSERT INTO agents (id, name, description, state, max_concurrent_runs, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'idle', ?4, ?5, ?5)",
            rusqlite::params![id, payload.name, payload.description, max_runs, now],
        )
        .map_err(|e| e.to_string())?;

        let agent = conn.query_row(
            "SELECT id, name, description, state, max_concurrent_runs, heartbeat_at, created_at, updated_at
             FROM agents WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok(Agent {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    state: row.get(3)?,
                    max_concurrent_runs: row.get(4)?,
                    heartbeat_at: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        )
        .map_err(|e| e.to_string())?;

        // Initialise workspace directory for the new agent
        let _ = workspace::init_agent_workspace(&agent.id);

        Ok(agent)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn update_agent(
    id: String,
    payload: UpdateAgent,
    db: tauri::State<'_, DbPool>,
) -> Result<Agent, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();

        if let Some(name) = &payload.name {
            conn.execute(
                "UPDATE agents SET name = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![name, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(desc) = &payload.description {
            conn.execute(
                "UPDATE agents SET description = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![desc, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(max_runs) = payload.max_concurrent_runs {
            conn.execute(
                "UPDATE agents SET max_concurrent_runs = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![max_runs, now, id],
            )
            .map_err(|e| e.to_string())?;
        }

        conn.query_row(
            "SELECT id, name, description, state, max_concurrent_runs, heartbeat_at, created_at, updated_at
             FROM agents WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok(Agent {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    state: row.get(3)?,
                    max_concurrent_runs: row.get(4)?,
                    heartbeat_at: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_agent(id: String, db: tauri::State<'_, DbPool>) -> Result<(), String> {
    if id == "default" {
        return Err("cannot delete the default agent".to_string());
    }
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM agents WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Cancel a specific run by its ID. Sends a cancellation signal via the executor registry.
#[tauri::command]
pub async fn cancel_run(run_id: String, db: tauri::State<'_, DbPool>) -> Result<(), String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        // Mark as cancelled in DB — the executor will check this if it hasn't already been signalled
        conn.execute(
            "UPDATE runs SET state = 'cancelled', finished_at = ?1 WHERE id = ?2 AND state IN ('pending', 'queued', 'running')",
            rusqlite::params![now, run_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
