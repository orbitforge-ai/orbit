use crate::app_context::AppContext;
use crate::db::cloud::CloudClientState;
use crate::events::emitter::emit_agent_config_changed_to_host;
use crate::executor::workspace::{self, AgentWorkspaceConfig, FileEntry};

/// Serialize model_config, persist to SQLite, and fire a cloud PATCH (fire-and-forget).
/// Called after any disk write that changes config.json or system_prompt.md.
pub(crate) async fn sync_model_config_to_cloud(
    agent_id: &str,
    pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
    cloud: CloudClientState,
) -> Result<(), String> {
    let aid = agent_id.to_string();
    let model_config_json =
        tokio::task::spawn_blocking(move || workspace::serialize_model_config(&aid))
            .await
            .map_err(|e| e.to_string())??;

    let mcj = model_config_json.clone();
    let aid2 = agent_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE agents SET model_config = ?1 WHERE id = ?2",
            rusqlite::params![mcj, aid2],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;

    if let Some(client) = cloud.get() {
        let aid3 = agent_id.to_string();
        tokio::spawn(async move {
            if let Err(e) = client
                .patch_agent_model_config(&aid3, &model_config_json)
                .await
            {
                tracing::warn!("cloud patch model_config {}: {}", aid3, e);
            }
        });
    }
    Ok(())
}

#[tauri::command]
pub fn get_workspace_path(agent_id: String) -> String {
    workspace::agent_dir(&agent_id)
        .to_string_lossy()
        .to_string()
}

#[tauri::command]
pub async fn init_agent_workspace(agent_id: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || workspace::init_agent_workspace(&agent_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_workspace_files(
    agent_id: String,
    path: Option<String>,
) -> Result<Vec<FileEntry>, String> {
    let rel = path.unwrap_or_else(|| ".".to_string());
    tokio::task::spawn_blocking(move || workspace::list_workspace_files(&agent_id, &rel))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn read_workspace_file(agent_id: String, path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || workspace::read_workspace_file(&agent_id, &path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn write_workspace_file(
    agent_id: String,
    path: String,
    content: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || workspace::write_workspace_file(&agent_id, &path, &content))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_workspace_file(agent_id: String, path: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || workspace::delete_workspace_file(&agent_id, &path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn create_workspace_dir(agent_id: String, path: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || workspace::create_workspace_dir(&agent_id, &path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn rename_workspace_entry(
    agent_id: String,
    from: String,
    to: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || workspace::rename_workspace_entry(&agent_id, &from, &to))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_agent_config(agent_id: String) -> Result<AgentWorkspaceConfig, String> {
    tokio::task::spawn_blocking(move || workspace::load_agent_config(&agent_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn update_agent_config(
    agent_id: String,
    config: AgentWorkspaceConfig,
    app: tauri::State<'_, AppContext>,
) -> Result<(), String> {
    update_agent_config_inner(agent_id, config, &app).await
}

async fn update_agent_config_inner(
    agent_id: String,
    config: AgentWorkspaceConfig,
    app: &AppContext,
) -> Result<(), String> {
    let role_id = config.role_id.clone();
    let agent_id_emit = agent_id.clone();
    let pool = app.db.0.clone();
    let cloud = app.cloud.clone();

    let agent_id_clone = agent_id.clone();
    tokio::task::spawn_blocking(move || workspace::save_agent_config(&agent_id_clone, &config))
        .await
        .map_err(|e| e.to_string())??;

    sync_model_config_to_cloud(&agent_id, pool, cloud).await?;

    emit_agent_config_changed_to_host(app.runtime.as_ref(), &agent_id_emit, role_id);
    Ok(())
}

/// Write system_prompt.md and sync model_config to SQLite + cloud.
#[tauri::command]
pub async fn update_system_prompt(
    agent_id: String,
    content: String,
    app: tauri::State<'_, AppContext>,
) -> Result<(), String> {
    update_system_prompt_inner(agent_id, content, &app).await
}

async fn update_system_prompt_inner(
    agent_id: String,
    content: String,
    app: &AppContext,
) -> Result<(), String> {
    let pool = app.db.0.clone();
    let cloud = app.cloud.clone();

    let agent_id_clone = agent_id.clone();
    let content_clone = content.clone();
    tokio::task::spawn_blocking(move || {
        workspace::write_workspace_file(&agent_id_clone, "system_prompt.md", &content_clone)
    })
    .await
    .map_err(|e| e.to_string())??;

    sync_model_config_to_cloud(&agent_id, pool, cloud).await
}

/// Returns a map of agentId → roleId for all agents that have a role configured.
/// Used by the sidebar to show role icons without fetching full configs.
#[tauri::command]
pub async fn list_agent_role_ids() -> Result<std::collections::HashMap<String, String>, String> {
    tokio::task::spawn_blocking(|| {
        let agents_root = workspace::agents_root();
        let mut map = std::collections::HashMap::new();
        let entries = match std::fs::read_dir(&agents_root) {
            Ok(e) => e,
            Err(_) => return Ok(map),
        };
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let agent_id = entry.file_name().to_string_lossy().to_string();
            if let Ok(config) = workspace::load_agent_config(&agent_id) {
                if let Some(role_id) = config.role_id {
                    map.insert(agent_id, role_id);
                }
            }
        }
        Ok(map)
    })
    .await
    .map_err(|e| e.to_string())?
}

mod http {
    use super::*;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct AgentIdArgs {
        agent_id: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ListArgs {
        agent_id: String,
        #[serde(default)]
        path: Option<String>,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct PathArgs {
        agent_id: String,
        path: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct WriteArgs {
        agent_id: String,
        path: String,
        content: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RenameArgs {
        agent_id: String,
        from: String,
        to: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UpdateConfigArgs {
        agent_id: String,
        config: AgentWorkspaceConfig,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UpdatePromptArgs {
        agent_id: String,
        content: String,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("get_workspace_path", |_ctx, args| async move {
            let a: AgentIdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            Ok(serde_json::Value::String(get_workspace_path(a.agent_id)))
        });
        reg.register("init_agent_workspace", |_ctx, args| async move {
            let a: AgentIdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            init_agent_workspace(a.agent_id).await?;
            Ok(serde_json::Value::Null)
        });
        reg.register("list_workspace_files", |_ctx, args| async move {
            let a: ListArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = list_workspace_files(a.agent_id, a.path).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("read_workspace_file", |_ctx, args| async move {
            let a: PathArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = read_workspace_file(a.agent_id, a.path).await?;
            Ok(serde_json::Value::String(r))
        });
        reg.register("write_workspace_file", |_ctx, args| async move {
            let a: WriteArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            write_workspace_file(a.agent_id, a.path, a.content).await?;
            Ok(serde_json::Value::Null)
        });
        reg.register("delete_workspace_file", |_ctx, args| async move {
            let a: PathArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            delete_workspace_file(a.agent_id, a.path).await?;
            Ok(serde_json::Value::Null)
        });
        reg.register("create_workspace_dir", |_ctx, args| async move {
            let a: PathArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            create_workspace_dir(a.agent_id, a.path).await?;
            Ok(serde_json::Value::Null)
        });
        reg.register("rename_workspace_entry", |_ctx, args| async move {
            let a: RenameArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            rename_workspace_entry(a.agent_id, a.from, a.to).await?;
            Ok(serde_json::Value::Null)
        });
        reg.register("get_agent_config", |_ctx, args| async move {
            let a: AgentIdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = get_agent_config(a.agent_id).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("update_agent_config", |ctx, args| async move {
            let a: UpdateConfigArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            update_agent_config_inner(a.agent_id, a.config, &ctx).await?;
            Ok(serde_json::Value::Null)
        });
        reg.register("update_system_prompt", |ctx, args| async move {
            let a: UpdatePromptArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            update_system_prompt_inner(a.agent_id, a.content, &ctx).await?;
            Ok(serde_json::Value::Null)
        });
        reg.register("list_agent_role_ids", |_ctx, _args| async move {
            let r = list_agent_role_ids().await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
    }
}

pub use http::register as register_http;
