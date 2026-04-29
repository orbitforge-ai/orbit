use crate::app_context::AppContext;
use crate::events::emitter::{
    emit_agent_config_changed_to_host, emit_agent_created_to_host, emit_agent_deleted_to_host,
    emit_agent_updated_to_host,
};
use crate::executor::workspace;
use crate::models::agent::{Agent, CreateAgent, UpdateAgent};
use rusqlite::{Connection, OptionalExtension, Transaction};
use serde_json::Value;

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

fn next_available_agent_id(
    conn: &Connection,
    name: &str,
    current_id: Option<&str>,
) -> Result<String, String> {
    let base_slug = workspace::slugify(name);
    let base_slug = if base_slug.is_empty() {
        "agent".to_string()
    } else {
        base_slug
    };

    let mut candidate = base_slug.clone();
    let mut suffix = 1;

    loop {
        let existing = conn
            .query_row(
                "SELECT id FROM agents WHERE id = ?1",
                rusqlite::params![candidate],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;

        match existing.as_deref() {
            None => return Ok(candidate),
            Some(existing_id) if Some(existing_id) == current_id => return Ok(candidate),
            Some(_) => {
                suffix += 1;
                candidate = format!("{}-{}", base_slug, suffix);
            }
        }
    }
}

fn replace_agent_ids_in_json(value: &mut Value, old_agent_id: &str, new_agent_id: &str) -> bool {
    match value {
        Value::Object(map) => {
            let mut changed = false;
            for (key, entry) in map.iter_mut() {
                if (key == "agentId" || key.ends_with("AgentId"))
                    && entry.as_str() == Some(old_agent_id)
                {
                    *entry = Value::String(new_agent_id.to_string());
                    changed = true;
                    continue;
                }
                changed |= replace_agent_ids_in_json(entry, old_agent_id, new_agent_id);
            }
            changed
        }
        Value::Array(items) => {
            let mut changed = false;
            for entry in items.iter_mut() {
                changed |= replace_agent_ids_in_json(entry, old_agent_id, new_agent_id);
            }
            changed
        }
        _ => false,
    }
}

fn rename_agent_workflow_references(
    tx: &Transaction<'_>,
    old_agent_id: &str,
    new_agent_id: &str,
    now: &str,
) -> Result<(), String> {
    let mut stmt = tx
        .prepare("SELECT id, graph, trigger_config FROM project_workflows")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    for row in rows {
        let (workflow_id, graph_json, trigger_config_json) = row.map_err(|e| e.to_string())?;
        let mut graph: Value = serde_json::from_str(&graph_json)
            .map_err(|e| format!("failed to parse workflow graph {}: {}", workflow_id, e))?;
        let mut trigger_config: Value =
            serde_json::from_str(&trigger_config_json).map_err(|e| {
                format!(
                    "failed to parse workflow trigger config {}: {}",
                    workflow_id, e
                )
            })?;

        let graph_changed = replace_agent_ids_in_json(&mut graph, old_agent_id, new_agent_id);
        let trigger_changed =
            replace_agent_ids_in_json(&mut trigger_config, old_agent_id, new_agent_id);

        if !graph_changed && !trigger_changed {
            continue;
        }

        let graph_json = serde_json::to_string(&graph).map_err(|e| {
            format!(
                "failed to serialize updated workflow graph {}: {}",
                workflow_id, e
            )
        })?;
        let trigger_config_json = serde_json::to_string(&trigger_config).map_err(|e| {
            format!(
                "failed to serialize updated workflow trigger config {}: {}",
                workflow_id, e
            )
        })?;

        tx.execute(
            "UPDATE project_workflows
                SET graph = ?1, trigger_config = ?2, version = version + 1, updated_at = ?3
              WHERE id = ?4",
            rusqlite::params![graph_json, trigger_config_json, now, workflow_id],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(())
}

fn rename_agent_references(
    tx: &Transaction<'_>,
    old_agent_id: &str,
    new_agent_id: &str,
    now: &str,
) -> Result<(), String> {
    for sql in [
        "UPDATE tasks SET agent_id = ?1 WHERE agent_id = ?2",
        "UPDATE runs SET agent_id = ?1 WHERE agent_id = ?2",
        "UPDATE chat_sessions SET agent_id = ?1 WHERE agent_id = ?2",
        "UPDATE agent_conversations SET agent_id = ?1 WHERE agent_id = ?2",
        "UPDATE agent_tasks SET agent_id = ?1 WHERE agent_id = ?2",
        "UPDATE project_agents SET agent_id = ?1 WHERE agent_id = ?2",
        "UPDATE bus_messages SET from_agent_id = ?1 WHERE from_agent_id = ?2",
        "UPDATE bus_messages SET to_agent_id = ?1 WHERE to_agent_id = ?2",
        "UPDATE bus_subscriptions SET subscriber_agent_id = ?1 WHERE subscriber_agent_id = ?2",
        "UPDATE bus_subscriptions SET source_agent_id = ?1 WHERE source_agent_id = ?2",
        "UPDATE work_items SET assignee_agent_id = ?1 WHERE assignee_agent_id = ?2",
        "UPDATE work_items SET created_by_agent_id = ?1 WHERE created_by_agent_id = ?2",
        "UPDATE work_item_comments SET author_agent_id = ?1 WHERE author_agent_id = ?2",
        "UPDATE memory_extraction_log SET agent_id = ?1 WHERE agent_id = ?2",
        "UPDATE channel_sessions SET agent_id = ?1 WHERE agent_id = ?2",
        "UPDATE plugin_entities SET created_by_agent_id = ?1 WHERE created_by_agent_id = ?2",
    ] {
        tx.execute(sql, rusqlite::params![new_agent_id, old_agent_id])
            .map_err(|e| e.to_string())?;
    }

    rename_agent_workflow_references(tx, old_agent_id, new_agent_id, now)
}

#[tauri::command]
pub async fn list_agents(app: tauri::State<'_, AppContext>) -> Result<Vec<Agent>, String> {
    app.repos.agents().list().await
}

#[tauri::command]
pub async fn create_agent(
    payload: CreateAgent,
    app: tauri::State<'_, AppContext>,
) -> Result<Agent, String> {
    create_agent_inner(payload, &app).await
}

async fn create_agent_inner(payload: CreateAgent, app: &AppContext) -> Result<Agent, String> {
    let role_id = payload.role_id.clone();
    let cloud = app.cloud.clone();
    let pool = app.db.0.clone();
    let (agent, model_config_json): (Agent, String) = tokio::task::spawn_blocking(move || -> Result<(Agent, String), String> {
        let initial_identity = payload.identity.clone();
        let initial_role_id = payload.role_id.clone();
        let initial_role_instructions = payload.role_system_instructions.clone();
        let conn = pool.get().map_err(|e| e.to_string())?;
        let id = next_available_agent_id(&conn, &payload.name, None)?;

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

    emit_agent_created_to_host(app.runtime.as_ref(), agent.clone(), role_id);
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
    id: String,
    payload: UpdateAgent,
    app: tauri::State<'_, AppContext>,
) -> Result<Agent, String> {
    update_agent_inner(id, payload, &app).await
}

async fn update_agent_inner(
    id: String,
    payload: UpdateAgent,
    app: &AppContext,
) -> Result<Agent, String> {
    let cloud = app.cloud.clone();
    let pool = app.db.0.clone();
    let (agent, previous_agent_id, role_id): (Agent, Option<String>, Option<String>) =
        tokio::task::spawn_blocking(move || -> Result<(Agent, Option<String>, Option<String>), String> {
        let mut conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        let next_id = match payload.name.as_deref() {
            Some(name) if id != "default" => next_available_agent_id(&conn, name, Some(&id))?,
            _ => id.clone(),
        };
        let slug_changed = next_id != id;

        if slug_changed {
            let active_run_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM runs
                      WHERE agent_id = ?1 AND state IN ('pending', 'queued', 'running')",
                    rusqlite::params![id],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if active_run_count > 0 {
                return Err("cannot rename an agent slug while it has active runs".to_string());
            }
        }

        let tx = conn.transaction().map_err(|e| e.to_string())?;
        tx.execute_batch("PRAGMA defer_foreign_keys = ON;")
            .map_err(|e| e.to_string())?;

        if slug_changed {
            tx.execute(
                "UPDATE agents SET id = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![next_id, now, id],
            )
            .map_err(|e| e.to_string())?;
            rename_agent_references(&tx, &id, &next_id, &now)?;
        }

        if let Some(name) = &payload.name {
            tx.execute(
                "UPDATE agents SET name = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![name, now, next_id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(desc) = &payload.description {
            tx.execute(
                "UPDATE agents SET description = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![desc, now, next_id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(max_runs) = payload.max_concurrent_runs {
            tx.execute(
                "UPDATE agents SET max_concurrent_runs = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![max_runs, now, next_id],
            )
            .map_err(|e| e.to_string())?;
        }

        let agent = tx
        .query_row(
            "SELECT id, name, description, state, max_concurrent_runs, heartbeat_at, created_at, updated_at
             FROM agents WHERE id = ?1",
            rusqlite::params![next_id],
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

        if slug_changed {
            workspace::rename_agent_root(&id, &next_id)?;
        }

        if let Err(err) = tx.commit() {
            if slug_changed {
                let _ = workspace::rename_agent_root(&next_id, &id);
            }
            return Err(err.to_string());
        }

        let role_id = workspace::load_agent_config(&agent.id)
            .ok()
            .and_then(|config| config.role_id);

        Ok((agent, slug_changed.then_some(id), role_id))
    })
    .await
    .map_err(|e| e.to_string())??;

    emit_agent_updated_to_host(
        app.runtime.as_ref(),
        agent.clone(),
        previous_agent_id.clone(),
    );
    if let Some(previous_agent_id) = previous_agent_id.as_deref() {
        emit_agent_config_changed_to_host(app.runtime.as_ref(), previous_agent_id, None);
        emit_agent_config_changed_to_host(app.runtime.as_ref(), &agent.id, role_id);
        cloud_delete!(cloud, "agents", previous_agent_id);
    }
    cloud_upsert_agent!(cloud, agent);
    Ok(agent)
}

#[tauri::command]
pub async fn delete_agent(id: String, app: tauri::State<'_, AppContext>) -> Result<(), String> {
    if id == "default" {
        return Err("cannot delete the default agent".to_string());
    }
    let cloud = app.cloud.clone();
    app.repos.agents().delete(&id).await?;
    emit_agent_deleted_to_host(app.runtime.as_ref(), &id);
    cloud_delete!(cloud, "agents", id);
    Ok(())
}

#[tauri::command]
pub async fn cancel_run(run_id: String, app: tauri::State<'_, AppContext>) -> Result<(), String> {
    app.repos.runs().cancel(&run_id).await
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
        payload: CreateAgent,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UpdateArgs {
        id: String,
        payload: UpdateAgent,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CancelArgs {
        run_id: String,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("list_agents", |ctx, _args| async move {
            let result = ctx.repos.agents().list().await?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        });
        reg.register("create_agent", |ctx, args| async move {
            let a: CreateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = create_agent_inner(a.payload, &ctx).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("update_agent", |ctx, args| async move {
            let a: UpdateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = update_agent_inner(a.id, a.payload, &ctx).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("delete_agent", |ctx, args| async move {
            let a: IdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            if a.id == "default" {
                return Err("cannot delete the default agent".to_string());
            }
            let cloud = ctx.cloud.clone();
            ctx.repos.agents().delete(&a.id).await?;
            emit_agent_deleted_to_host(ctx.runtime.as_ref(), &a.id);
            cloud_delete!(cloud, "agents", a.id);
            Ok(serde_json::Value::Null)
        });
        reg.register("cancel_run", |ctx, args| async move {
            let a: CancelArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            ctx.repos.runs().cancel(&a.run_id).await?;
            Ok(serde_json::Value::Null)
        });
    }
}

pub use http::register as register_http;
