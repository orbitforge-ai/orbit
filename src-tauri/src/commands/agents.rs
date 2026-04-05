use crate::db::cloud::CloudClientState;
use crate::db::DbPool;
use crate::events::emitter::{emit_agent_created, emit_agent_deleted, emit_agent_updated};
use crate::executor::workspace;
use crate::models::agent::{Agent, CreateAgent, UpdateAgent};

/// Fire-and-forget helper: clone the Arc, spawn, log failures.
macro_rules! cloud_upsert_agent {
    ($cloud:expr, $agent:expr) => {
        if let Some(client) = $cloud.get() {
            let a = $agent.clone();
            tokio::spawn(async move {
                if let Err(e) = client.upsert_agent(&a, None).await {
                    tracing::warn!("cloud upsert agent: {}", e);
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
    app: tauri::AppHandle,
    payload: CreateAgent,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<Agent, String> {
    let role_id = payload.role_id.clone();
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let (agent, model_config_json): (Agent, String) = tokio::task::spawn_blocking(move || -> Result<(Agent, String), String> {
        let initial_identity = payload.identity.clone();
        let initial_role_id = payload.role_id.clone();
        let initial_role_instructions = payload.role_system_instructions.clone();
        let conn = pool.get().map_err(|e| e.to_string())?;
        let base_slug = workspace::slugify(&payload.name);
        let base_slug = if base_slug.is_empty() { "agent".to_string() } else { base_slug };

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

        workspace::init_agent_workspace(&agent.id)?;
        if initial_identity.is_some() || initial_role_id.is_some() {
            let mut config = workspace::load_agent_config(&agent.id)?;
            if let Some(identity) = initial_identity {
                config.identity = workspace::normalize_agent_identity(&identity);
            }
            if let Some(role_id) = initial_role_id {
                config.role_id = Some(role_id);
            }
            if let Some(ri) = initial_role_instructions {
                config.role_system_instructions = Some(ri);
            }
            workspace::save_agent_config(&agent.id, &config)?;
        }

        // Persist the initial model_config blob to SQLite so push_local_data is current
        let model_config_json = workspace::serialize_model_config(&agent.id)
            .unwrap_or_else(|_| "{}".to_string());
        conn.execute(
            "UPDATE agents SET model_config = ?1 WHERE id = ?2",
            rusqlite::params![model_config_json, agent.id],
        )
        .map_err(|e| e.to_string())?;

        Ok((agent, model_config_json))
    })
    .await
    .map_err(|e| e.to_string())??;

    emit_agent_created(&app, agent.clone(), role_id);
    // Include model_config in the initial upsert to avoid a race with a separate PATCH
    if let Some(client) = cloud.get() {
        let a = agent.clone();
        let mcj = model_config_json.clone();
        tokio::spawn(async move {
            if let Err(e) = client.upsert_agent(&a, Some(&mcj)).await {
                tracing::warn!("cloud upsert agent on create: {}", e);
            }
        });
    }
    Ok(agent)
}

#[tauri::command]
pub async fn update_agent(
    app: tauri::AppHandle,
    id: String,
    payload: UpdateAgent,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<Agent, String> {
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let agent: Agent = tokio::task::spawn_blocking(move || -> Result<Agent, String> {
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
    .map_err(|e| e.to_string())??;

    emit_agent_updated(&app, agent.clone());
    cloud_upsert_agent!(cloud, agent);
    Ok(agent)
}

#[tauri::command]
pub async fn delete_agent(
    app: tauri::AppHandle,
    id: String,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
    if id == "default" {
        return Err("cannot delete the default agent".to_string());
    }
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let id_clone = id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM agents WHERE id = ?1",
            rusqlite::params![id_clone],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;

    emit_agent_deleted(&app, &id);
    cloud_delete!(cloud, "agents", id);
    Ok(())
}

#[tauri::command]
pub async fn cancel_run(run_id: String, db: tauri::State<'_, DbPool>) -> Result<(), String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
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
