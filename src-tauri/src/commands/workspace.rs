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
