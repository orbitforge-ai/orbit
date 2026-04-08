use std::fs;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::executor::http::validate_url_for_ssrf;
use crate::executor::workspace::agent_dir;

const CHANNEL_REQUEST_TIMEOUT_SECS: u64 = 20;

/// Supported external messaging channel types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ChannelType {
    Slack,
    Discord,
    Webhook,
}

/// A configured external messaging channel owned by a single agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelConfig {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub channel_type: ChannelType,
    /// Webhook URL for Slack / Discord / generic webhook.
    pub webhook_url: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChannelsFile {
    #[serde(default)]
    pub channels: Vec<ChannelConfig>,
}

fn channels_path(agent_id: &str) -> std::path::PathBuf {
    agent_dir(agent_id).join("channels.json")
}

/// Load all channels configured for an agent. Returns an empty list if the
/// file does not exist yet.
pub fn load_channels(agent_id: &str) -> Result<Vec<ChannelConfig>, String> {
    let path = channels_path(agent_id);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content =
        fs::read_to_string(&path).map_err(|e| format!("failed to read channels: {}", e))?;
    let file: ChannelsFile =
        serde_json::from_str(&content).map_err(|e| format!("failed to parse channels: {}", e))?;
    Ok(file.channels)
}

/// Look up a channel by id or name (case-insensitive).
pub fn find_channel(channels: &[ChannelConfig], needle: &str) -> Option<ChannelConfig> {
    let lowered = needle.to_lowercase();
    channels
        .iter()
        .find(|c| c.id == needle || c.name.to_lowercase() == lowered)
        .cloned()
}

/// Send a plain-text message to the given channel. Returns the channel name
/// that received the message on success.
pub async fn send_to_channel(config: &ChannelConfig, message: &str) -> Result<String, String> {
    if !config.enabled {
        return Err(format!("channel '{}' is disabled", config.name));
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

    #[test]
    fn find_channel_by_name_is_case_insensitive() {
        let channels = vec![ChannelConfig {
            id: "ch1".into(),
            name: "Ops".into(),
            channel_type: ChannelType::Slack,
            webhook_url: "https://example.com".into(),
            enabled: true,
        }];
        assert!(find_channel(&channels, "ops").is_some());
        assert!(find_channel(&channels, "OPS").is_some());
        assert!(find_channel(&channels, "ch1").is_some());
        assert!(find_channel(&channels, "missing").is_none());
    }
}
