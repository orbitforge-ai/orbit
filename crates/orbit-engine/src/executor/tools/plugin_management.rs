//! `plugin_management` agent tool — drives the plugin lifecycle from within
//! an agent session. Used by the built-in `create-plugin` skill to scaffold
//! and install a new plugin end-to-end.
//!
//! Actions that mutate plugin state (`install_from_directory`, `reload`,
//! `uninstall`, `enable`, `disable`) are gated on the
//! `developer.pluginDevMode` global setting; regular users can only call
//! `list`, `status`, `logs`, `oauth_status`.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::executor::global_settings::load_global_settings;
use crate::executor::llm_provider::ToolDefinition;
use crate::plugins;

use super::{context::ToolExecutionContext, ToolHandler};

pub struct PluginManagementTool;

#[async_trait]
impl ToolHandler for PluginManagementTool {
    fn name(&self) -> &'static str {
        "plugin_management"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Manage Orbit plugins from an agent. Actions: `list` (all installed), \
                 `status` (single plugin state), `logs` (recent stderr), `oauth_status` \
                 (connected providers), and when developer.pluginDevMode is enabled: \
                 `install_from_directory`, `enable`, `disable`, `reload`, `uninstall`. \
                 Used by the `create-plugin` skill to scaffold, install, and iterate."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["action"],
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "list", "status", "logs", "oauth_status",
                            "install_from_directory", "enable", "disable",
                            "reload", "uninstall"
                        ]
                    },
                    "plugin_id": { "type": "string" },
                    "path": { "type": "string", "description": "Directory path for install_from_directory." },
                    "tail_lines": { "type": "integer", "minimum": 1, "maximum": 1000 }
                }
            }),
        }
    }

    async fn execute(
        &self,
        _ctx: &ToolExecutionContext,
        input: &Value,
        app: &tauri::AppHandle,
        _run_id: &str,
    ) -> Result<(String, bool), String> {
        let action = input
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing `action`".to_string())?;
        let manager = plugins::from_state(app);

        let result = match action {
            "list" => json!({ "plugins": manager.list() }),

            "status" => {
                let plugin_id = require_plugin_id(input)?;
                let summary = manager
                    .list()
                    .into_iter()
                    .find(|p| p.id == plugin_id)
                    .ok_or_else(|| format!("plugin {:?} not installed", plugin_id))?;
                json!(summary)
            }

            "logs" => {
                let plugin_id = require_plugin_id(input)?;
                let tail_lines = input
                    .get("tail_lines")
                    .and_then(Value::as_i64)
                    .unwrap_or(200)
                    .max(1)
                    .min(1000) as usize;
                json!({ "log": manager.runtime.log_tail(&plugin_id, tail_lines) })
            }

            "oauth_status" => {
                let plugin_id = require_plugin_id(input)?;
                let manifest = manager
                    .manifest(&plugin_id)
                    .ok_or_else(|| format!("plugin {:?} not installed", plugin_id))?;
                let providers: Vec<_> = manifest
                    .oauth_providers
                    .iter()
                    .map(|p| {
                        let connected = plugins::oauth::get_secret(
                            &plugin_id,
                            &format!("oauth.{}.access", p.id),
                        )
                        .is_ok();
                        json!({ "id": p.id, "name": p.name, "connected": connected })
                    })
                    .collect();
                json!({ "providers": providers })
            }

            "install_from_directory" => {
                require_dev_mode()?;
                let path = input
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "`install_from_directory` requires `path`".to_string())?;
                let manifest =
                    manager.install_from_directory(app, &std::path::PathBuf::from(path))?;
                json!({ "installed": manifest.id })
            }

            "enable" => {
                require_dev_mode()?;
                let plugin_id = require_plugin_id(input)?;
                manager.set_enabled(app, &plugin_id, true)?;
                json!({ "enabled": plugin_id })
            }

            "disable" => {
                require_dev_mode()?;
                let plugin_id = require_plugin_id(input)?;
                manager.set_enabled(app, &plugin_id, false)?;
                json!({ "disabled": plugin_id })
            }

            "reload" => {
                require_dev_mode()?;
                let plugin_id = require_plugin_id(input)?;
                manager.reload(app, &plugin_id)?;
                json!({ "reloaded": plugin_id })
            }

            "uninstall" => {
                require_dev_mode()?;
                let plugin_id = require_plugin_id(input)?;
                manager.uninstall(app, &plugin_id)?;
                json!({ "uninstalled": plugin_id })
            }

            other => return Err(format!("unknown action {:?}", other)),
        };

        let text = serde_json::to_string(&result)
            .map_err(|e| format!("failed to serialise result: {}", e))?;
        Ok((text, false))
    }
}

fn require_plugin_id(input: &Value) -> Result<String, String> {
    input
        .get("plugin_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "`plugin_id` required".to_string())
}

fn require_dev_mode() -> Result<(), String> {
    let settings = load_global_settings();
    if !settings.developer.plugin_dev_mode {
        return Err("This action requires developer.pluginDevMode = true in settings.json".into());
    }
    Ok(())
}
