use crate::executor::workspace::{self, AgentWorkspaceConfig, FileEntry};

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
    agent_id: String,
    config: AgentWorkspaceConfig,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || workspace::save_agent_config(&agent_id, &config))
        .await
        .map_err(|e| e.to_string())?
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
