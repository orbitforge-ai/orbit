use serde::{Deserialize, Serialize};

/// Per-agent subscription linking an external-provider channel or thread to
/// an agent. When a plugin emits a `TriggerEventPayload` that matches a
/// binding, the dispatcher spawns an agent run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChannelBinding {
    pub id: String,
    pub plugin_id: String,
    pub provider_channel_id: String,
    #[serde(default)]
    pub provider_thread_id: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "default_true")]
    pub auto_respond: bool,
    #[serde(default)]
    pub mention_only: bool,
}

fn default_true() -> bool {
    true
}

impl ChannelBinding {
    /// Match a binding against an inbound event's channel/thread identifiers.
    /// A binding with no `providerThreadId` matches messages on the channel
    /// regardless of thread; with a thread id set it only matches that thread.
    pub fn matches(&self, plugin_id: &str, channel_id: &str, thread_id: Option<&str>) -> bool {
        if self.plugin_id != plugin_id {
            return false;
        }
        if self.provider_channel_id != channel_id {
            return false;
        }
        match (&self.provider_thread_id, thread_id) {
            (None, _) => true,
            (Some(a), Some(b)) => a == b,
            (Some(_), None) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(plugin: &str, ch: &str, thread: Option<&str>) -> ChannelBinding {
        ChannelBinding {
            id: "b1".into(),
            plugin_id: plugin.into(),
            provider_channel_id: ch.into(),
            provider_thread_id: thread.map(str::to_string),
            label: None,
            auto_respond: true,
            mention_only: false,
        }
    }

    #[test]
    fn channel_only_binding_matches_any_thread() {
        let b = binding("com.orbit.discord", "C1", None);
        assert!(b.matches("com.orbit.discord", "C1", None));
        assert!(b.matches("com.orbit.discord", "C1", Some("T1")));
    }

    #[test]
    fn thread_scoped_binding_rejects_parent_channel() {
        let b = binding("com.orbit.discord", "C1", Some("T1"));
        assert!(!b.matches("com.orbit.discord", "C1", None));
        assert!(b.matches("com.orbit.discord", "C1", Some("T1")));
        assert!(!b.matches("com.orbit.discord", "C1", Some("T2")));
    }

    #[test]
    fn plugin_id_must_match() {
        let b = binding("com.orbit.discord", "C1", None);
        assert!(!b.matches("com.orbit.slack", "C1", None));
    }
}
