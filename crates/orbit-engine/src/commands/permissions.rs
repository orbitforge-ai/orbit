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

mod http {
    use super::*;
    use crate::executor::permissions::PermissionRegistry;
    use tauri::Manager;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RespondArgs {
        request_id: String,
        response: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SaveRuleArgs {
        #[serde(default)]
        agent_id: Option<String>,
        rule: PermissionRule,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct DeleteRuleArgs {
        #[serde(default)]
        agent_id: Option<String>,
        rule_id: String,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("respond_to_permission", |ctx, args| async move {
            let app = ctx.app()?;
            let a: RespondArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            respond_to_permission(a.request_id, a.response, app.state::<PermissionRegistry>())
                .await?;
            Ok(serde_json::Value::Null)
        });
        reg.register("save_permission_rule", |_ctx, args| async move {
            let a: SaveRuleArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            save_permission_rule(a.agent_id, a.rule).await?;
            Ok(serde_json::Value::Null)
        });
        reg.register("delete_permission_rule", |_ctx, args| async move {
            let a: DeleteRuleArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            delete_permission_rule(a.agent_id, a.rule_id).await?;
            Ok(serde_json::Value::Null)
        });
    }
}

pub use http::register as register_http;
