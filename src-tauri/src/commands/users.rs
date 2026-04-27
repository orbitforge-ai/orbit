use crate::db::cloud::CloudClientState;
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
pub async fn create_user(
    name: String,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<User, String> {
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let user: User = tokio::task::spawn_blocking(move || -> Result<User, String> {
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
    .map_err(|e| e.to_string())??;

    if let Some(client) = cloud.get() {
        let u = user.clone();
        tokio::spawn(async move {
            if let Err(e) = client.upsert_user(&u).await {
                tracing::warn!("cloud upsert user: {}", e);
            }
        });
    }
    Ok(user)
}

#[tauri::command]
pub async fn get_active_user(active_user: tauri::State<'_, ActiveUser>) -> Result<String, String> {
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

mod http {
    use tauri::Manager;
    use super::*;
    use crate::db::cloud::CloudClientState;
    use crate::db::DbPool;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CreateArgs { name: String }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SetActiveArgs { user_id: String }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("list_users", |ctx, _args| async move {
            let app = ctx.app()?;
            let r = list_users(app.state::<DbPool>()).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("create_user", |ctx, args| async move {
            let app = ctx.app()?;
            let a: CreateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = create_user(a.name, app.state::<DbPool>(), app.state::<CloudClientState>()).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("get_active_user", |ctx, _args| async move {
            let app = ctx.app()?;
            let r = get_active_user(app.state::<ActiveUser>()).await?;
            Ok(serde_json::Value::String(r))
        });
        reg.register("set_active_user", |ctx, args| async move {
            let app = ctx.app()?;
            let a: SetActiveArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            set_active_user(a.user_id, app.state::<ActiveUser>(), app.state::<DbPool>()).await?;
            Ok(serde_json::Value::Null)
        });
    }
}

pub use http::register as register_http;
