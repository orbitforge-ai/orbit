use ulid::Ulid;

use crate::db::DbPool;
use crate::models::bus::{BusMessage, BusSubscription, CreateBusSubscription};

#[tauri::command]
pub async fn list_bus_messages(
    agent_id: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<BusMessage>, String> {
    let pool = db.0.clone();
    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;

        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match agent_id {
            Some(ref aid) => (
                "SELECT id, from_agent_id, from_run_id, to_agent_id, to_run_id, kind, event_type, payload, status, created_at
                 FROM bus_messages
                 WHERE from_agent_id = ?1 OR to_agent_id = ?1
                 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3".to_string(),
                vec![Box::new(aid.clone()), Box::new(limit), Box::new(offset)],
            ),
            None => (
                "SELECT id, from_agent_id, from_run_id, to_agent_id, to_run_id, kind, event_type, payload, status, created_at
                 FROM bus_messages
                 ORDER BY created_at DESC LIMIT ?1 OFFSET ?2".to_string(),
                vec![Box::new(limit), Box::new(offset)],
            ),
        };

        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let messages = stmt
            .query_map(params_refs.as_slice(), |row| {
                let payload_str: String = row.get(7)?;
                Ok(BusMessage {
                    id: row.get(0)?,
                    from_agent_id: row.get(1)?,
                    from_run_id: row.get(2)?,
                    to_agent_id: row.get(3)?,
                    to_run_id: row.get(4)?,
                    kind: row.get(5)?,
                    event_type: row.get(6)?,
                    payload: serde_json::from_str(&payload_str).unwrap_or(serde_json::Value::Null),
                    status: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        Ok(messages)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_bus_subscriptions(
    agent_id: Option<String>,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<BusSubscription>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;

        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match agent_id {
            Some(ref aid) => (
                "SELECT id, subscriber_agent_id, source_agent_id, event_type, task_id, payload_template, enabled, max_chain_depth, created_at, updated_at
                 FROM bus_subscriptions
                 WHERE subscriber_agent_id = ?1 OR source_agent_id = ?1
                 ORDER BY created_at DESC".to_string(),
                vec![Box::new(aid.clone())],
            ),
            None => (
                "SELECT id, subscriber_agent_id, source_agent_id, event_type, task_id, payload_template, enabled, max_chain_depth, created_at, updated_at
                 FROM bus_subscriptions
                 ORDER BY created_at DESC".to_string(),
                vec![],
            ),
        };

        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let subs = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(BusSubscription {
                    id: row.get(0)?,
                    subscriber_agent_id: row.get(1)?,
                    source_agent_id: row.get(2)?,
                    event_type: row.get(3)?,
                    task_id: row.get(4)?,
                    payload_template: row.get(5)?,
                    enabled: row.get(6)?,
                    max_chain_depth: row.get(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        Ok(subs)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn create_bus_subscription(
    payload: CreateBusSubscription,
    db: tauri::State<'_, DbPool>,
) -> Result<BusSubscription, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let id = Ulid::new().to_string();
        let now = chrono::Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO bus_subscriptions (id, subscriber_agent_id, source_agent_id, event_type, task_id, payload_template, enabled, max_chain_depth, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?8)",
            rusqlite::params![
                id,
                payload.subscriber_agent_id,
                payload.source_agent_id,
                payload.event_type,
                payload.task_id,
                payload.payload_template,
                payload.max_chain_depth,
                now,
            ],
        )
        .map_err(|e| e.to_string())?;

        Ok(BusSubscription {
            id,
            subscriber_agent_id: payload.subscriber_agent_id,
            source_agent_id: payload.source_agent_id,
            event_type: payload.event_type,
            task_id: payload.task_id,
            payload_template: payload.payload_template,
            enabled: true,
            max_chain_depth: payload.max_chain_depth,
            created_at: now.clone(),
            updated_at: now,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn toggle_bus_subscription(
    id: String,
    enabled: bool,
    db: tauri::State<'_, DbPool>,
) -> Result<(), String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE bus_subscriptions SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![enabled, now, id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_bus_subscription(
    id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<(), String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM bus_subscriptions WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
