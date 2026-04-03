use crate::db::cloud::CloudClientState;
use crate::db::DbPool;
use crate::executor::permissions::{PermissionRegistry, PermissionResponse};
use crate::executor::workspace::{self, PermissionRule};

#[tauri::command]
pub async fn respond_to_permission(
    request_id: String,
    response: String,
    registry: tauri::State<'_, PermissionRegistry>,
) -> Result<(), String> {
    let permission_response = match response.as_str() {
        "allow" => PermissionResponse::Allow,
        "always_allow" => PermissionResponse::AlwaysAllow,
        "deny" => PermissionResponse::Deny,
        _ => return Err(format!("Invalid permission response: '{}'", response)),
    };
    registry.resolve(&request_id, permission_response).await
}

#[tauri::command]
pub async fn save_permission_rule(
    agent_id: String,
    rule: PermissionRule,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
    let pool = db.0.clone();
    let cloud = cloud.inner().clone();
    let agent_id_clone = agent_id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut config = workspace::load_agent_config(&agent_id_clone)?;
        config.permission_rules.retain(|r| !(r.tool == rule.tool && r.pattern == rule.pattern));
        config.permission_rules.push(rule);
        workspace::save_agent_config(&agent_id_clone, &config)
    })
    .await
    .map_err(|e| e.to_string())??;
    super::workspace::sync_model_config_to_cloud(&agent_id, pool, cloud).await
}

#[tauri::command]
pub async fn delete_permission_rule(
    agent_id: String,
    rule_id: String,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
    let pool = db.0.clone();
    let cloud = cloud.inner().clone();
    let agent_id_clone = agent_id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut config = workspace::load_agent_config(&agent_id_clone)?;
        config.permission_rules.retain(|r| r.id != rule_id);
        workspace::save_agent_config(&agent_id_clone, &config)
    })
    .await
    .map_err(|e| e.to_string())??;
    super::workspace::sync_model_config_to_cloud(&agent_id, pool, cloud).await
}
