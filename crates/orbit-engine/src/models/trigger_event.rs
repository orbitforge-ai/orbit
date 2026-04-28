use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Normalized inbound event emitted by a plugin (Discord/Slack/...) and
/// delivered to the core dispatcher over the per-plugin JSON-RPC socket.
///
/// The same payload is also the future cloud-relay contract — a relay
/// delivering replayed events hits `trigger.emit` with these exact fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerEventPayload {
    /// Provider-unique id for this message. Used for dedupe so a replay does
    /// not re-fire the same run.
    pub event_id: String,
    pub plugin_id: String,
    /// Namespaced trigger kind — must start with
    /// `trigger.<plugin-slug>.` (enforced by the manifest validator).
    pub kind: String,
    pub channel: TriggerEventChannel,
    pub user: TriggerEventUser,
    pub text: String,
    #[serde(default)]
    pub mentions: Vec<String>,
    pub received_at: String,
    /// Provider-specific blob for advanced workflow nodes.
    #[serde(default)]
    pub raw: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerEventChannel {
    pub id: String,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// Discord guild id / Slack team id, if applicable.
    #[serde(default)]
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerEventUser {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub bot: bool,
}

impl TriggerEventPayload {
    /// Parse and structurally validate a payload received on the
    /// `trigger.emit` JSON-RPC method. Rejects events whose `pluginId` does
    /// not match the plugin whose socket delivered them, or whose `kind`
    /// does not carry the plugin's expected trigger namespace.
    pub fn from_rpc_params(
        params: Value,
        plugin_id: &str,
        plugin_slug: &str,
    ) -> Result<Self, String> {
        let payload: TriggerEventPayload =
            serde_json::from_value(params).map_err(|e| format!("invalid payload: {}", e))?;

        if payload.plugin_id != plugin_id {
            return Err(format!(
                "pluginId {:?} does not match socket owner {:?}",
                payload.plugin_id, plugin_id
            ));
        }
        let expected_prefix = format!("trigger.{}.", plugin_slug);
        if !payload.kind.starts_with(&expected_prefix) {
            return Err(format!(
                "kind {:?} must start with {:?}",
                payload.kind, expected_prefix
            ));
        }
        if payload.event_id.trim().is_empty() {
            return Err("eventId is required".into());
        }
        if payload.channel.id.trim().is_empty() {
            return Err("channel.id is required".into());
        }
        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_params() -> Value {
        json!({
            "eventId": "m-123",
            "pluginId": "com.orbit.discord",
            "kind": "trigger.com_orbit_discord.message",
            "channel": { "id": "C1", "name": "general" },
            "user": { "id": "U1", "displayName": "alice", "bot": false },
            "text": "hello",
            "mentions": [],
            "receivedAt": "2026-04-19T00:00:00Z"
        })
    }

    #[test]
    fn accepts_valid_payload() {
        let p = TriggerEventPayload::from_rpc_params(
            valid_params(),
            "com.orbit.discord",
            "com_orbit_discord",
        )
        .unwrap();
        assert_eq!(p.text, "hello");
        assert_eq!(p.channel.id, "C1");
    }

    #[test]
    fn rejects_plugin_id_mismatch() {
        let err = TriggerEventPayload::from_rpc_params(
            valid_params(),
            "com.orbit.slack",
            "com_orbit_slack",
        )
        .unwrap_err();
        assert!(err.contains("does not match"));
    }

    #[test]
    fn rejects_wrong_kind_prefix() {
        let mut v = valid_params();
        v["kind"] = json!("trigger.com_orbit_slack.message");
        let err = TriggerEventPayload::from_rpc_params(v, "com.orbit.discord", "com_orbit_discord")
            .unwrap_err();
        assert!(err.contains("must start with"));
    }

    #[test]
    fn rejects_empty_event_id() {
        let mut v = valid_params();
        v["eventId"] = json!("");
        let err = TriggerEventPayload::from_rpc_params(v, "com.orbit.discord", "com_orbit_discord")
            .unwrap_err();
        assert!(err.contains("eventId"));
    }
}
