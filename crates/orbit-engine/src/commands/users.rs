use crate::app_context::AppContext;
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
pub async fn list_users(app: tauri::State<'_, AppContext>) -> Result<Vec<User>, String> {
    app.repos.users().list().await
}

#[tauri::command]
pub async fn create_user(
    name: String,
    app: tauri::State<'_, AppContext>,
) -> Result<User, String> {
    let cloud = app.cloud.clone();
    let user = app.repos.users().create(name).await?;
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
    app: tauri::State<'_, AppContext>,
) -> Result<(), String> {
    if !app.repos.users().exists(&user_id).await? {
        return Err(format!("User '{}' not found", user_id));
    }
    let mut guard = active_user.0.write().await;
    *guard = user_id;
    Ok(())
}

mod http {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CreateArgs {
        name: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SetActiveArgs {
        user_id: String,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("list_users", |ctx, _args| async move {
            let r = ctx.repos.users().list().await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("create_user", |ctx, args| async move {
            let a: CreateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            let user = ctx.repos.users().create(a.name).await?;
            if let Some(client) = cloud.get() {
                let u = user.clone();
                tokio::spawn(async move {
                    if let Err(e) = client.upsert_user(&u).await {
                        tracing::warn!("cloud upsert user: {}", e);
                    }
                });
            }
            serde_json::to_value(user).map_err(|e| e.to_string())
        });
        reg.register("get_active_user", |ctx, _args| async move {
            Ok(serde_json::Value::String(ctx.active_user.get().await))
        });
        reg.register("set_active_user", |ctx, args| async move {
            let a: SetActiveArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            if !ctx.repos.users().exists(&a.user_id).await? {
                return Err(format!("User '{}' not found", a.user_id));
            }
            let mut guard = ctx.active_user.0.write().await;
            *guard = a.user_id;
            Ok(serde_json::Value::Null)
        });
    }
}

pub use http::register as register_http;
