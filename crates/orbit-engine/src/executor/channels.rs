use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::executor::global_settings;
use crate::executor::http::validate_url_for_ssrf;

const CHANNEL_REQUEST_TIMEOUT_SECS: u64 = 20;

/// Supported external messaging channel types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ChannelType {
    Slack,
    Discord,
    Webhook,
}

/// Delivery mode for a channel. `Webhook` is the legacy one-way POST; `Bot`
/// indicates a plugin owns this channel and is able to both listen to and
/// reply on it. Legacy persisted rows (pre-bot) default to `Webhook`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChannelMode {
    Webhook,
    Bot,
}

impl Default for ChannelMode {
    fn default() -> Self {
        ChannelMode::Webhook
    }
}

/// Opaque pointer to a credential stored in the OS keychain. Core never
/// unpacks the secret; it passes `service`/`account` to the keychain layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialRef {
    pub service: String,
    pub account: String,
}

/// A configured external messaging channel owned by a single agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelConfig {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub channel_type: ChannelType,
    /// Webhook URL for Slack / Discord / generic webhook. Optional when the
    /// channel is bot-backed — a plugin owns delivery in that case. Retained
    /// as `String` (not `Option<String>`) and defaulted to empty for
    /// backward compatibility with persisted settings.
    #[serde(default)]
    pub webhook_url: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub mode: ChannelMode,
    /// Plugin id that owns this channel when `mode == Bot`.
    #[serde(default)]
    pub plugin_id: Option<String>,
    /// Provider-native channel id (Discord snowflake / Slack C-id).
    #[serde(default)]
    pub provider_channel_id: Option<String>,
    /// Provider-native thread id (Discord snowflake / Slack `thread_ts`).
    #[serde(default)]
    pub provider_thread_id: Option<String>,
    /// Pointer to a Keychain entry holding a bot token or OAuth access token.
    #[serde(default)]
    pub credential_ref: Option<CredentialRef>,
}

fn default_true() -> bool {
    true
}

impl ChannelConfig {
    /// Is this channel bot-backed (plugin owns both send and receive)?
    pub fn is_bot(&self) -> bool {
        matches!(self.mode, ChannelMode::Bot)
    }
}

/// Load all configured channels from the global settings file.
pub fn load_channels() -> Vec<ChannelConfig> {
    global_settings::load_global_settings().channels
}

/// Look up a channel by id or name (case-insensitive).
pub fn find_channel(channels: &[ChannelConfig], needle: &str) -> Option<ChannelConfig> {
    let lowered = needle.to_lowercase();
    channels
        .iter()
        .find(|c| c.id == needle || c.name.to_lowercase() == lowered)
        .cloned()
}

/// Send a plain-text message to the given channel via its configured webhook.
///
/// Bot-backed channels are **not** handled here — the caller (the `message`
/// tool) is responsible for detecting `mode == Bot` and routing through the
/// owning plugin's `send_message` tool. This function remains the webhook
/// path only.
pub async fn send_to_channel(config: &ChannelConfig, message: &str) -> Result<String, String> {
    if !config.enabled {
        return Err(format!("channel '{}' is disabled", config.name));
    }
    if config.is_bot() {
        return Err(format!(
            "channel '{}' is bot-backed; route through the owning plugin instead of the webhook path",
            config.name
        ));
    }
    if config.webhook_url.trim().is_empty() {
        return Err(format!(
            "channel '{}' has no webhook URL configured",
            config.name
        ));
    }

    validate_url_for_ssrf(&config.webhook_url).await?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(CHANNEL_REQUEST_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| format!("failed to create HTTP client: {}", e))?;

    let payload = build_payload(&config.channel_type, message);

    let response = client
        .post(&config.webhook_url)
        .header(
            reqwest::header::USER_AGENT,
            "Orbit/0.1 (+https://github.com/orbitforge-ai/orbit)",
        )
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("channel request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let trimmed: String = body.chars().take(500).collect();
        return Err(format!(
            "channel '{}' returned {}{}",
            config.name,
            status,
            if trimmed.is_empty() {
                String::new()
            } else {
                format!(": {}", trimmed)
            }
        ));
    }

    info!(
        channel = %config.name,
        channel_type = ?config.channel_type,
        "sent message to external channel"
    );

    Ok(config.name.clone())
}

/// Build the provider-specific JSON payload for a webhook message.
fn build_payload(channel_type: &ChannelType, message: &str) -> serde_json::Value {
    match channel_type {
        // Slack incoming webhooks accept `{"text": "..."}`.
        ChannelType::Slack => serde_json::json!({ "text": message }),
        // Discord incoming webhooks accept `{"content": "..."}` (max 2000 chars).
        ChannelType::Discord => {
            let truncated: String = message.chars().take(1950).collect();
            serde_json::json!({ "content": truncated })
        }
        // Generic webhook: send a simple JSON envelope the receiver can parse.
        ChannelType::Webhook => serde_json::json!({
            "text": message,
            "source": "orbit",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slack_payload_uses_text_field() {
        let payload = build_payload(&ChannelType::Slack, "hello");
        assert_eq!(payload["text"], "hello");
    }

    #[test]
    fn discord_payload_uses_content_field_and_truncates() {
        let long = "x".repeat(3000);
        let payload = build_payload(&ChannelType::Discord, &long);
        let content = payload["content"].as_str().unwrap();
        assert!(content.len() <= 1950);
    }

    #[test]
    fn webhook_payload_has_text_and_source() {
        let payload = build_payload(&ChannelType::Webhook, "hi");
        assert_eq!(payload["text"], "hi");
        assert_eq!(payload["source"], "orbit");
    }

    fn webhook_channel(id: &str, name: &str) -> ChannelConfig {
        ChannelConfig {
            id: id.into(),
            name: name.into(),
            channel_type: ChannelType::Slack,
            webhook_url: "https://example.com".into(),
            enabled: true,
            mode: ChannelMode::Webhook,
            plugin_id: None,
            provider_channel_id: None,
            provider_thread_id: None,
            credential_ref: None,
        }
    }

    #[test]
    fn find_channel_by_name_is_case_insensitive() {
        let channels = vec![webhook_channel("ch1", "Ops")];
        assert!(find_channel(&channels, "ops").is_some());
        assert!(find_channel(&channels, "OPS").is_some());
        assert!(find_channel(&channels, "ch1").is_some());
        assert!(find_channel(&channels, "missing").is_none());
    }

    #[test]
    fn legacy_channel_row_deserialises_as_webhook_mode() {
        let legacy = serde_json::json!({
            "id": "ch1",
            "name": "Ops",
            "type": "slack",
            "webhookUrl": "https://hooks.slack.com/x"
        });
        let parsed: ChannelConfig = serde_json::from_value(legacy).unwrap();
        assert_eq!(parsed.mode, ChannelMode::Webhook);
        assert!(!parsed.is_bot());
        assert!(parsed.plugin_id.is_none());
        assert!(parsed.enabled);
    }

    #[tokio::test]
    async fn send_rejects_bot_mode_channel() {
        let mut ch = webhook_channel("ch1", "Ops");
        ch.mode = ChannelMode::Bot;
        let err = send_to_channel(&ch, "hi").await.unwrap_err();
        assert!(err.contains("bot-backed"));
    }
}
