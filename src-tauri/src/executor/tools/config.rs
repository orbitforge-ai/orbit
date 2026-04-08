use serde::Serialize;
use serde_json::{json, Value};

use crate::events::emitter::emit_agent_config_changed;
use crate::executor::{llm_provider::ToolDefinition, workspace};

use super::{context::ToolExecutionContext, ToolHandler};

const MODIFIABLE_SETTINGS: &[&str] = &[
    "model",
    "temperature",
    "maxIterations",
    "maxTotalTokens",
    "memoryEnabled",
];
const BLOCKED_SETTINGS: &[&str] = &[
    // These now live in global settings and cannot be mutated via the
    // per-agent config tool at all.
    "permissionMode",
    "permissionRules",
    "allowedTools",
    "webSearchProvider",
    "disabledSkills",
    "disabledTools",
    "defaultChannelId",
];

pub struct ConfigTool;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigInfoResult {
    agent_id: String,
    workspace: String,
    chain_depth: i64,
    is_sub_agent: bool,
    session_id: Option<String>,
    run_id: Option<String>,
    current_worktree: Option<serde_json::Value>,
}

#[async_trait::async_trait]
impl ToolHandler for ConfigTool {
    fn name(&self) -> &'static str {
        "config"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Get or set your agent configuration. Use 'get' to read a setting, 'set' to change one, 'list' to see all settings, and 'info' for agent metadata.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["get", "set", "list", "info"],
                        "description": "Action to perform"
                    },
                    "setting": {
                        "type": "string",
                        "description": "Setting name for get/set, such as 'model' or 'temperature'"
                    },
                    "value": {
                        "description": "New value for the setting when action = 'set'"
                    }
                },
                "required": ["action"]
            }),
        }
    }

    async fn execute(
        &self,
        ctx: &ToolExecutionContext,
        input: &serde_json::Value,
        app: &tauri::AppHandle,
        _run_id: &str,
    ) -> Result<(String, bool), String> {
        let action = input["action"]
            .as_str()
            .ok_or("config: missing 'action' field")?;

        match action {
            "list" => {
                let config = workspace::load_agent_config(&ctx.agent_id)?;
                let result = serde_json::to_string_pretty(&config)
                    .map_err(|e| format!("config: failed to serialize config: {}", e))?;
                Ok((result, false))
            }
            "get" => {
                let setting = input["setting"]
                    .as_str()
                    .ok_or("config: missing 'setting' for get action")?;
                let config = workspace::load_agent_config(&ctx.agent_id)?;
                let value = get_setting_value(&config, setting)?;
                let result = serde_json::to_string_pretty(&json!({
                    "setting": setting,
                    "value": value,
                }))
                .map_err(|e| format!("config: failed to serialize result: {}", e))?;
                Ok((result, false))
            }
            "set" => {
                let setting = input["setting"]
                    .as_str()
                    .ok_or("config: missing 'setting' for set action")?;
                let value = input
                    .get("value")
                    .ok_or("config: missing 'value' for set action")?;
                ensure_setting_is_mutable(setting)?;

                let mut config = workspace::load_agent_config(&ctx.agent_id)?;
                apply_setting_value(&mut config, setting, value)?;
                workspace::save_agent_config(&ctx.agent_id, &config)?;
                sync_agent_config_side_effects(ctx, app, &config).await?;

                let result = serde_json::to_string_pretty(&json!({
                    "status": "updated",
                    "setting": setting,
                    "value": get_setting_value(&config, setting)?,
                }))
                .map_err(|e| format!("config: failed to serialize result: {}", e))?;
                Ok((result, false))
            }
            "info" => {
                let current_worktree = ctx.current_worktree().map(|state| {
                    json!({
                        "name": state.name,
                        "branch": state.branch,
                        "path": state.path,
                    })
                });
                let info = ConfigInfoResult {
                    agent_id: ctx.agent_id.clone(),
                    workspace: ctx.workspace_root().to_string_lossy().to_string(),
                    chain_depth: ctx.chain_depth,
                    is_sub_agent: ctx.is_sub_agent,
                    session_id: ctx.current_session_id.clone(),
                    run_id: ctx.current_run_id.clone(),
                    current_worktree,
                };
                let result = serde_json::to_string_pretty(&info)
                    .map_err(|e| format!("config: failed to serialize info: {}", e))?;
                Ok((result, false))
            }
            other => Err(format!("config: unknown action '{}'", other)),
        }
    }
}

fn get_setting_value(
    config: &workspace::AgentWorkspaceConfig,
    setting: &str,
) -> Result<Value, String> {
    let value = serde_json::to_value(config)
        .map_err(|e| format!("config: failed to serialize config: {}", e))?;
    value
        .get(setting)
        .cloned()
        .ok_or_else(|| format!("config: unknown setting '{}'", setting))
}

fn ensure_setting_is_mutable(setting: &str) -> Result<(), String> {
    if BLOCKED_SETTINGS.contains(&setting) {
        return Err(format!(
            "config: setting '{}' is blocked for safety and cannot be changed at runtime",
            setting
        ));
    }
    if !MODIFIABLE_SETTINGS.contains(&setting) {
        return Err(format!(
            "config: setting '{}' is not supported by the config tool",
            setting
        ));
    }
    Ok(())
}

fn apply_setting_value(
    config: &mut workspace::AgentWorkspaceConfig,
    setting: &str,
    value: &Value,
) -> Result<(), String> {
    match setting {
        "model" => {
            config.model = value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or("config: model must be a non-empty string")?;
        }
        "temperature" => {
            config.temperature = value
                .as_f64()
                .ok_or("config: temperature must be a number")?;
        }
        "maxIterations" => {
            config.max_iterations = value
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value > 0)
                .ok_or("config: maxIterations must be a positive integer")?;
        }
        "maxTotalTokens" => {
            config.max_total_tokens = value
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value > 0)
                .ok_or("config: maxTotalTokens must be a positive integer")?;
        }
        "memoryEnabled" => {
            config.memory_enabled = value
                .as_bool()
                .ok_or("config: memoryEnabled must be a boolean")?;
        }
        _ => {
            return Err(format!(
                "config: setting '{}' is not supported by the config tool",
                setting
            ));
        }
    }
    Ok(())
}

async fn sync_agent_config_side_effects(
    ctx: &ToolExecutionContext,
    app: &tauri::AppHandle,
    config: &workspace::AgentWorkspaceConfig,
) -> Result<(), String> {
    if let Some(db) = &ctx.db {
        let model_config_json = workspace::serialize_model_config(&ctx.agent_id)?;
        let pool = db.0.clone();
        let agent_id = ctx.agent_id.clone();
        let model_config_json_for_db = model_config_json.clone();

        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE agents SET model_config = ?1 WHERE id = ?2",
                rusqlite::params![model_config_json_for_db, agent_id],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())??;

        if let Some(client) = ctx.cloud_client.clone() {
            let agent_id = ctx.agent_id.clone();
            tokio::spawn(async move {
                if let Err(e) = client
                    .patch_agent_model_config(&agent_id, &model_config_json)
                    .await
                {
                    tracing::warn!("cloud patch model_config {}: {}", agent_id, e);
                }
            });
        }
    }

    emit_agent_config_changed(app, &ctx.agent_id, config.role_id.clone());
    Ok(())
}
