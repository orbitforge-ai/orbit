use crate::db::DbPool;
use crate::models::user::User;

/// Managed state holding the currently active user ID.
#[derive(Clone)]
pub struct ActiveUser(pub std::sync::Arc<tokio::sync::RwLock<String>>);

impl ActiveUser {
    pub fn new(user_id: String) -> Self {
        Self(std::sync::Arc::new(tokio::sync::RwLock::new(user_id)))
    }

    pub async fn get(&self) -> String {
        self.0.read().await.clone()
    }
}

#[tauri::command]
pub async fn list_users(db: tauri::State<'_, DbPool>) -> Result<Vec<User>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, name, is_default, created_at FROM users ORDER BY created_at ASC")
            .map_err(|e| e.to_string())?;
        let users = stmt
            .query_map([], |row| {
                Ok(User {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    is_default: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(users)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn create_user(name: String, db: tauri::State<'_, DbPool>) -> Result<User, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let id = ulid::Ulid::new().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO users (id, name, is_default, created_at) VALUES (?1, ?2, 0, ?3)",
            rusqlite::params![id, name, now],
        )
        .map_err(|e| e.to_string())?;
        Ok(User {
            id,
            name,
            is_default: false,
            created_at: now,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_active_user(
    active_user: tauri::State<'_, ActiveUser>,
) -> Result<String, String> {
    Ok(active_user.get().await)
}

#[tauri::command]
pub async fn set_active_user(
    user_id: String,
    active_user: tauri::State<'_, ActiveUser>,
    db: tauri::State<'_, DbPool>,
) -> Result<(), String> {
    // Verify user exists
    let pool = db.0.clone();
    let uid = user_id.clone();
    let exists = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM users WHERE id = ?1",
                rusqlite::params![uid],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        Ok::<bool, String>(count > 0)
    })
    .await
    .map_err(|e| e.to_string())??;

    if !exists {
        return Err(format!("User '{}' not found", user_id));
    }

    let mut guard = active_user.0.write().await;
    *guard = user_id;
    Ok(())
}
