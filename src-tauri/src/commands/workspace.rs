use crate::db::cloud::CloudClientState;
use crate::db::DbPool;
use crate::events::emitter::emit_agent_config_changed;
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
            if let Err(e) = client.patch_agent_model_config(&aid3, &model_config_json).await {
                tracing::warn!("cloud patch model_config {}: {}", aid3, e);
            }
        });
    }
    Ok(())
}

#[tauri::command]
pub fn get_workspace_path(agent_id: String) -> String {
    workspace::agent_dir(&agent_id).to_string_lossy().to_string()
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
    tokio::task::spawn_blocking(move || {
        workspace::write_workspace_file(&agent_id, &path, &content)
    })
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
pub async fn get_agent_config(agent_id: String) -> Result<AgentWorkspaceConfig, String> {
    tokio::task::spawn_blocking(move || workspace::load_agent_config(&agent_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn update_agent_config(
    app: tauri::AppHandle,
    agent_id: String,
    config: AgentWorkspaceConfig,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
    let role_id = config.role_id.clone();
    let agent_id_emit = agent_id.clone();
    let pool = db.0.clone();
    let cloud = cloud.inner().clone();

    let agent_id_clone = agent_id.clone();
    tokio::task::spawn_blocking(move || workspace::save_agent_config(&agent_id_clone, &config))
        .await
        .map_err(|e| e.to_string())??;

    sync_model_config_to_cloud(&agent_id, pool, cloud).await?;

    emit_agent_config_changed(&app, &agent_id_emit, role_id);
    Ok(())
}

/// Write system_prompt.md and sync model_config to SQLite + cloud.
#[tauri::command]
pub async fn update_system_prompt(
    agent_id: String,
    content: String,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
    let pool = db.0.clone();
    let cloud = cloud.inner().clone();

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
