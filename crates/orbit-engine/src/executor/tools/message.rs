use serde_json::json;
use tauri::Manager;
use tracing::{info, warn};

use crate::executor::channels::{self, ChannelConfig, ChannelMode, ChannelType};
use crate::executor::global_settings;
use crate::executor::llm_provider::ToolDefinition;
use crate::executor::workspace;
use crate::plugins;
use crate::triggers::reply_registry::{ReplyChannel, ReplyRegistry};

use super::{context::ToolExecutionContext, ToolHandler};

const MAX_MESSAGE_LEN: usize = 10_000;
/// Tool name a bot-backed plugin must expose to receive outbound sends from
/// the core `message` tool.
const PLUGIN_SEND_TOOL: &str = "send_message";

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
        app: &tauri::AppHandle,
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
                // Channel resolution order:
                //   1. Explicit `channel` in the tool call.
                //   2. Ambient `reply_channel` from the trigger dispatcher
                //      (the bot replies in the same place it was mentioned).
                //   3. Agent's configured `default_channel_id`.
                let explicit_channel = input["channel"].as_str().map(str::to_string);
                let resolved_channel = if let Some(raw) = explicit_channel {
                    channels::find_channel(&channels, &raw).ok_or_else(|| {
                        format!(
                            "message: channel '{}' not found. Use action='list' to see configured channels.",
                            raw
                        )
                    })?
                } else if let Some(reply) = lookup_reply_channel(app, run_id) {
                    synthesize_reply_channel(&reply)
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
                    mode = ?resolved_channel.mode,
                    "agent tool: message send"
                );

                let result = if resolved_channel.is_bot() {
                    send_via_plugin(app, &resolved_channel, text).await
                } else {
                    channels::send_to_channel(&resolved_channel, text)
                        .await
                        .map(|name| format!("Message delivered to channel '{}'.", name))
                };
                match result {
                    Ok(msg) => Ok((msg, false)),
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

/// Consult the ambient reply registry for a reply target bound to this run.
/// Returns `None` when the run was not spawned from a trigger event or the
/// registry is not installed in app state.
fn lookup_reply_channel(app: &tauri::AppHandle, run_id: &str) -> Option<ReplyChannel> {
    let state = app.try_state::<ReplyRegistry>()?;
    state.inner().get(run_id)
}

/// Build a synthetic bot-mode [`ChannelConfig`] from a trigger-dispatched
/// reply context. Not persisted — only used to drive the existing send
/// pipeline through `send_via_plugin`.
fn synthesize_reply_channel(reply: &ReplyChannel) -> ChannelConfig {
    ChannelConfig {
        id: format!(
            "reply:{}:{}{}",
            reply.plugin_id,
            reply.provider_channel_id,
            reply
                .provider_thread_id
                .as_deref()
                .map(|t| format!(":{}", t))
                .unwrap_or_default()
        ),
        name: format!("reply to {}", reply.provider_channel_id),
        channel_type: ChannelType::Webhook, // unused in bot path
        webhook_url: String::new(),
        enabled: true,
        mode: ChannelMode::Bot,
        plugin_id: Some(reply.plugin_id.clone()),
        provider_channel_id: Some(reply.provider_channel_id.clone()),
        provider_thread_id: reply.provider_thread_id.clone(),
        credential_ref: None,
    }
}

/// Route an outbound message through the plugin that owns a bot-backed
/// channel. The plugin is expected to expose a tool named `send_message`
/// that accepts `{ channelId, threadId?, text }`.
async fn send_via_plugin(
    app: &tauri::AppHandle,
    channel: &ChannelConfig,
    text: &str,
) -> Result<String, String> {
    let plugin_id = channel.plugin_id.as_deref().ok_or_else(|| {
        format!(
            "channel '{}' is bot-backed but has no pluginId configured",
            channel.name
        )
    })?;
    let provider_channel_id = channel.provider_channel_id.as_deref().ok_or_else(|| {
        format!(
            "channel '{}' is bot-backed but has no providerChannelId configured",
            channel.name
        )
    })?;
    let manager = plugins::from_state(app);
    let manifest = manager
        .manifest(plugin_id)
        .ok_or_else(|| format!("plugin '{}' is not installed", plugin_id))?;
    if !manager.is_enabled(plugin_id) {
        return Err(format!("plugin '{}' is disabled", plugin_id));
    }
    if !manifest.tools.iter().any(|t| t.name == PLUGIN_SEND_TOOL) {
        return Err(format!(
            "plugin '{}' does not expose a '{}' tool",
            plugin_id, PLUGIN_SEND_TOOL
        ));
    }

    let args = json!({
        "channelId": provider_channel_id,
        "threadId": channel.provider_thread_id,
        "text": text,
    });
    let extra_env = plugins::oauth::build_env_for_subprocess(&manifest);
    let send_result = manager
        .runtime
        .call_tool(&manifest, PLUGIN_SEND_TOOL, &args, &extra_env)
        .await;

    // Clear any active typing indicator as soon as the reply lands. Otherwise
    // Discord keeps the "Bot is typing…" display for ~10s after the last
    // pulse, which flashes back in after the message is posted.
    if manifest.tools.iter().any(|t| t.name == "stop_typing") {
        let stop_args = json!({
            "channelId": provider_channel_id,
            "threadId": channel.provider_thread_id,
        });
        if let Err(err) = manager
            .runtime
            .call_tool(&manifest, "stop_typing", &stop_args, &extra_env)
            .await
        {
            tracing::warn!(plugin_id, "stop_typing after send failed: {}", err);
        }
    }

    send_result.map(|_| format!("Message delivered to channel '{}'.", channel.name))
}
