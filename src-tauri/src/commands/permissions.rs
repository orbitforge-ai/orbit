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
) -> Result<(), String> {
    let mut config = workspace::load_agent_config(&agent_id)?;
    // Avoid duplicates: remove any existing rule with the same tool + pattern
    config.permission_rules.retain(|r| !(r.tool == rule.tool && r.pattern == rule.pattern));
    config.permission_rules.push(rule);
    workspace::save_agent_config(&agent_id, &config)
}

#[tauri::command]
pub async fn delete_permission_rule(
    agent_id: String,
    rule_id: String,
) -> Result<(), String> {
    let mut config = workspace::load_agent_config(&agent_id)?;
    config.permission_rules.retain(|r| r.id != rule_id);
    workspace::save_agent_config(&agent_id, &config)
}
