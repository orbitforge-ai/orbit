use crate::auth::{AuthMode, AuthState};
use crate::commands::users::ActiveUser;
use crate::executor::memory::MemoryEntry;
use crate::memory_service::MemoryServiceState;

fn client(
    state: &tauri::State<'_, Option<MemoryServiceState>>,
) -> Result<crate::executor::memory::MemoryClient, String> {
    state
        .as_ref()
        .map(|s| s.client.clone())
        .ok_or_else(|| "Memory service is not available".to_string())
}

async fn resolve_user_id(auth: &AuthState, active_user: &ActiveUser) -> String {
    match auth.get().await {
        AuthMode::Cloud(session) => session.user_id,
        _ => active_user.get().await,
    }
}

#[tauri::command]
pub async fn search_memories(
    query: String,
    memory_type: Option<String>,
    limit: Option<u32>,
    memory_state: tauri::State<'_, Option<MemoryServiceState>>,
    active_user: tauri::State<'_, ActiveUser>,
    auth: tauri::State<'_, AuthState>,
) -> Result<Vec<MemoryEntry>, String> {
    let c = client(&memory_state)?;
    let user_id = resolve_user_id(&auth, &active_user).await;
    let limit = limit.unwrap_or(10).min(50);
    c.search_memories(&query, &user_id, memory_type.as_deref(), limit)
        .await
}

#[tauri::command]
pub async fn list_memories(
    memory_type: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
    memory_state: tauri::State<'_, Option<MemoryServiceState>>,
    active_user: tauri::State<'_, ActiveUser>,
    auth: tauri::State<'_, AuthState>,
) -> Result<Vec<MemoryEntry>, String> {
    let c = client(&memory_state)?;
    let user_id = resolve_user_id(&auth, &active_user).await;
    let limit = limit.unwrap_or(50).min(200);
    let offset = offset.unwrap_or(0);
    c.list_memories(&user_id, memory_type.as_deref(), limit, offset)
        .await
}

#[tauri::command]
pub async fn add_memory(
    text: String,
    memory_type: String,
    memory_state: tauri::State<'_, Option<MemoryServiceState>>,
    active_user: tauri::State<'_, ActiveUser>,
    auth: tauri::State<'_, AuthState>,
) -> Result<Vec<MemoryEntry>, String> {
    let c = client(&memory_state)?;
    let user_id = resolve_user_id(&auth, &active_user).await;
    c.add_memory(&text, &memory_type, &user_id, None).await
}

#[tauri::command]
pub async fn delete_memory(
    memory_id: String,
    memory_state: tauri::State<'_, Option<MemoryServiceState>>,
) -> Result<(), String> {
    let c = client(&memory_state)?;
    c.delete_memory(&memory_id).await
}

#[tauri::command]
pub async fn update_memory(
    memory_id: String,
    text: String,
    memory_state: tauri::State<'_, Option<MemoryServiceState>>,
) -> Result<MemoryEntry, String> {
    let c = client(&memory_state)?;
    c.update_memory(&memory_id, Some(&text), None).await
}

#[tauri::command]
pub async fn get_memory_health(
    memory_state: tauri::State<'_, Option<MemoryServiceState>>,
) -> Result<bool, String> {
    Ok(memory_state.is_some())
}

mod http {
    use super::*;
    use crate::auth::AuthState;
    use crate::commands::users::ActiveUser;
    use crate::memory_service::MemoryServiceState;
    use tauri::Manager;

    #[derive(serde::Deserialize, Default)]
    #[serde(default, rename_all = "camelCase")]
    struct SearchArgs {
        query: String,
        memory_type: Option<String>,
        limit: Option<u32>,
    }
    #[derive(serde::Deserialize, Default)]
    #[serde(default, rename_all = "camelCase")]
    struct ListArgs {
        memory_type: Option<String>,
        limit: Option<u32>,
        offset: Option<u32>,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct AddArgs {
        text: String,
        memory_type: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct DeleteArgs {
        memory_id: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UpdateArgs {
        memory_id: String,
        text: String,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("search_memories", |ctx, args| async move {
            let app = ctx.app()?;
            let a: SearchArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = search_memories(
                a.query,
                a.memory_type,
                a.limit,
                app.state::<Option<MemoryServiceState>>(),
                app.state::<ActiveUser>(),
                app.state::<AuthState>(),
            )
            .await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("list_memories", |ctx, args| async move {
            let app = ctx.app()?;
            let a: ListArgs = if args.is_null() {
                Default::default()
            } else {
                serde_json::from_value(args).map_err(|e| e.to_string())?
            };
            let r = list_memories(
                a.memory_type,
                a.limit,
                a.offset,
                app.state::<Option<MemoryServiceState>>(),
                app.state::<ActiveUser>(),
                app.state::<AuthState>(),
            )
            .await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("add_memory", |ctx, args| async move {
            let app = ctx.app()?;
            let a: AddArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = add_memory(
                a.text,
                a.memory_type,
                app.state::<Option<MemoryServiceState>>(),
                app.state::<ActiveUser>(),
                app.state::<AuthState>(),
            )
            .await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("delete_memory", |ctx, args| async move {
            let app = ctx.app()?;
            let a: DeleteArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            delete_memory(a.memory_id, app.state::<Option<MemoryServiceState>>()).await?;
            Ok(serde_json::Value::Null)
        });
        reg.register("update_memory", |ctx, args| async move {
            let app = ctx.app()?;
            let a: UpdateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = update_memory(
                a.memory_id,
                a.text,
                app.state::<Option<MemoryServiceState>>(),
            )
            .await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("get_memory_health", |ctx, _args| async move {
            let app = ctx.app()?;
            let r = get_memory_health(app.state::<Option<MemoryServiceState>>()).await?;
            Ok(serde_json::Value::Bool(r))
        });
    }
}

pub use http::register as register_http;
