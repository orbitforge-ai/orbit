use serde_json::json;
use tracing::{info, warn};

use crate::executor::channels::{self, ChannelType};
use crate::executor::global_settings;
use crate::executor::llm_provider::ToolDefinition;
use crate::executor::workspace;

use super::{context::ToolExecutionContext, ToolHandler};

const MAX_MESSAGE_LEN: usize = 10_000;

pub struct MessageTool;

#[async_trait::async_trait]
impl ToolHandler for MessageTool {
    fn name(&self) -> &'static str {
        "message"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Send a message to a configured external channel (Slack, Discord, webhook). Use action='list' to see available channels and action='send' to deliver a message. Channels are configured globally in Settings. When action='send' omits 'channel', the agent's default outbound channel is used.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["send", "list"],
                        "description": "The action to perform. 'list' returns the configured channels. 'send' delivers a message to one channel."
                    },
                    "channel": {
                        "type": "string",
                        "description": "Channel name or id to deliver to. Optional when action='send' — if omitted, the agent's configured default outbound channel is used."
                    },
                    "text": {
                        "type": "string",
                        "description": "The message body. Required when action='send'. Max 10000 characters."
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
        _app: &tauri::AppHandle,
        run_id: &str,
    ) -> Result<(String, bool), String> {
        let action = input["action"].as_str().unwrap_or("send");

        let channels = channels::load_channels();

        match action {
            "list" => {
                if channels.is_empty() {
                    return Ok((
                        "No external channels are configured. Add one in Settings → Channels."
                            .to_string(),
                        false,
                    ));
                }
                let mut lines = vec![format!("{} channel(s) configured:", channels.len())];
                for ch in &channels {
                    let kind = match ch.channel_type {
                        ChannelType::Slack => "slack",
                        ChannelType::Discord => "discord",
                        ChannelType::Webhook => "webhook",
                    };
                    let status = if ch.enabled { "enabled" } else { "disabled" };
                    lines.push(format!("- {} [{}] ({})", ch.name, kind, status));
                }
                Ok((lines.join("\n"), false))
            }
            "send" => {
                // Resolve the channel: explicit 'channel' wins, otherwise fall
                // back to the agent's default outbound channel id.
                let explicit_channel = input["channel"].as_str().map(str::to_string);
                let resolved_channel = if let Some(raw) = explicit_channel {
                    channels::find_channel(&channels, &raw).ok_or_else(|| {
                        format!(
                            "message: channel '{}' not found. Use action='list' to see configured channels.",
                            raw
                        )
                    })?
                } else {
                    let ws_config = workspace::load_agent_config(&ctx.agent_id)
                        .map_err(|e| format!("message: {}", e))?;
                    let default_id = ws_config.default_channel_id.ok_or_else(|| {
                        "message: no 'channel' provided and this agent has no default outbound channel. Set one in the agent config, or pass 'channel' explicitly.".to_string()
                    })?;
                    global_settings::find_channel_by_id(&default_id).ok_or_else(|| {
                        format!(
                            "message: default channel '{}' no longer exists in global settings. Set a new default or pass 'channel' explicitly.",
                            default_id
                        )
                    })?
                };

                let text = input["text"]
                    .as_str()
                    .ok_or("message: missing 'text' field for action=send")?;

                if text.trim().is_empty() {
                    return Err("message: 'text' cannot be empty".to_string());
                }
                if text.chars().count() > MAX_MESSAGE_LEN {
                    return Err(format!(
                        "message: 'text' exceeds {} character limit",
                        MAX_MESSAGE_LEN
                    ));
                }

                info!(
                    run_id = run_id,
                    agent_id = %ctx.agent_id,
                    channel = %resolved_channel.name,
                    "agent tool: message send"
                );

                match channels::send_to_channel(&resolved_channel, text).await {
                    Ok(name) => Ok((format!("Message delivered to channel '{}'.", name), false)),
                    Err(err) => {
                        warn!(
                            run_id = run_id,
                            channel = %resolved_channel.name,
                            error = %err,
                            "message tool: delivery failed"
                        );
                        Err(format!("message: {}", err))
                    }
                }
            }
            other => Err(format!(
                "message: unknown action '{}'. Expected 'send' or 'list'.",
                other
            )),
        }
    }
}
