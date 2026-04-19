use crate::executor::global_settings;
use crate::executor::permissions::{PermissionRegistry, PermissionResponse};
use crate::executor::workspace::PermissionRule;

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

/// Persist a permission rule to the global settings file.
///
/// `agent_id` is accepted for one release so older frontend callers do not
/// break mid-upgrade, but it is logged and ignored. Remove it in the release
/// after this one.
#[tauri::command]
pub async fn save_permission_rule(
    agent_id: Option<String>,
    rule: PermissionRule,
) -> Result<(), String> {
    if let Some(id) = agent_id.as_deref() {
        tracing::debug!(
            agent_id = %id,
            "save_permission_rule: agent_id argument is ignored; rules are now global"
        );
    }
    tokio::task::spawn_blocking(move || global_settings::save_global_permission_rule(rule))
        .await
        .map_err(|e| e.to_string())??;
    Ok(())
}

/// Delete a permission rule from the global settings file.
///
/// `agent_id` is accepted for one release for backwards compatibility and
/// ignored. Remove in the next release.
#[tauri::command]
pub async fn delete_permission_rule(
    agent_id: Option<String>,
    rule_id: String,
) -> Result<(), String> {
    if let Some(id) = agent_id.as_deref() {
        tracing::debug!(
            agent_id = %id,
            "delete_permission_rule: agent_id argument is ignored; rules are now global"
        );
    }
    tokio::task::spawn_blocking(move || global_settings::delete_global_permission_rule(&rule_id))
        .await
        .map_err(|e| e.to_string())??;
    Ok(())
}
