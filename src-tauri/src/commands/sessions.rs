use ulid::Ulid;

use crate::db::DbPool;
use crate::models::session::{CreateSession, Session, UpdateSession};

fn row_to_session(row: &rusqlite::Row) -> rusqlite::Result<Session> {
    let env_str: String = row.get(3)?;
    let tags_str: String = row.get(4)?;
    Ok(Session {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        environment: serde_json::from_str(&env_str).unwrap_or_default(),
        tags: serde_json::from_str(&tags_str).unwrap_or_default(),
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

#[tauri::command]
pub async fn list_sessions(db: tauri::State<'_, DbPool>) -> Result<Vec<Session>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, name, description, environment, tags, created_at, updated_at
                 FROM sessions ORDER BY created_at DESC",
            )
            .map_err(|e| e.to_string())?;

        let sessions = stmt
            .query_map([], |row| row_to_session(row))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        Ok(sessions)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_session(id: String, db: tauri::State<'_, DbPool>) -> Result<Session, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT id, name, description, environment, tags, created_at, updated_at
             FROM sessions WHERE id = ?1",
            rusqlite::params![id],
            |row| row_to_session(row),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn create_session(
    payload: CreateSession,
    db: tauri::State<'_, DbPool>,
) -> Result<Session, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let id = Ulid::new().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let env_str =
            serde_json::to_string(&payload.environment.unwrap_or_default()).map_err(|e| e.to_string())?;
        let tags_str =
            serde_json::to_string(&payload.tags.unwrap_or_default()).map_err(|e| e.to_string())?;

        conn.execute(
            "INSERT INTO sessions (id, name, description, environment, tags, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            rusqlite::params![id, payload.name, payload.description, env_str, tags_str, now],
        )
        .map_err(|e| e.to_string())?;

        conn.query_row(
            "SELECT id, name, description, environment, tags, created_at, updated_at
             FROM sessions WHERE id = ?1",
            rusqlite::params![id],
            |row| row_to_session(row),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn update_session(
    id: String,
    payload: UpdateSession,
    db: tauri::State<'_, DbPool>,
) -> Result<Session, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();

        if let Some(name) = &payload.name {
            conn.execute(
                "UPDATE sessions SET name = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![name, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(desc) = &payload.description {
            conn.execute(
                "UPDATE sessions SET description = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![desc, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(env) = &payload.environment {
            let env_str = serde_json::to_string(env).map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE sessions SET environment = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![env_str, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(tags) = &payload.tags {
            let tags_str = serde_json::to_string(tags).map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE sessions SET tags = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![tags_str, now, id],
            )
            .map_err(|e| e.to_string())?;
        }

        conn.query_row(
            "SELECT id, name, description, environment, tags, created_at, updated_at
             FROM sessions WHERE id = ?1",
            rusqlite::params![id],
            |row| row_to_session(row),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_session(id: String, db: tauri::State<'_, DbPool>) -> Result<(), String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM sessions WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
