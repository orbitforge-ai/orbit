//! Fan-out dispatcher for inbound trigger events.
//!
//! A single instance of [`Dispatcher`] is shared by every plugin's core-API
//! socket handler. It is cheap to clone (internally `Arc`) and safe to call
//! concurrently; each event is processed independently.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::{debug, info};

use crate::models::channel_binding::ChannelBinding;
use crate::models::trigger_event::TriggerEventPayload;

/// How long a seen `eventId` is remembered for dedupe purposes.
const DEDUPE_TTL: Duration = Duration::from_secs(5 * 60);
/// Cap on the dedupe ring to bound memory when a plugin misbehaves.
const DEDUPE_MAX_ENTRIES: usize = 4096;

/// The result of one dispatch — how many workflows and agents matched, and
/// whether the event was a dedupe hit. Returned to the calling plugin so it
/// can log or surface an ack.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchResult {
    pub duplicate: bool,
    pub matched_workflows: usize,
    pub matched_agents: usize,
}

/// Pluggable lookup surface so the dispatcher can be unit-tested without
/// booting the workflow orchestrator or agent runner. The real implementation
/// in `lib.rs`'s setup wires this to the `DbPool` + `PluginManager`.
#[async_trait]
pub trait DispatchBindings: Send + Sync {
    /// Return every agent whose `listen_bindings` match the event's
    /// `(pluginId, channel, thread?)`. Returns `(agent_id, binding)` pairs.
    async fn matching_agent_bindings(
        &self,
        plugin_id: &str,
        channel_id: &str,
        thread_id: Option<&str>,
    ) -> Vec<(String, ChannelBinding)>;

    /// Return every enabled workflow whose `triggerKind` and
    /// `triggerConfig` match the event's kind and channel/thread.
    /// Returns workflow ids.
    async fn matching_workflow_ids(&self, event: &TriggerEventPayload) -> Vec<String>;

    /// Fire the workflow with the given trigger event as its seed input.
    /// Called for each matched workflow id.
    fn run_workflow_from_event(&self, workflow_id: &str, event: &TriggerEventPayload);

    /// Spawn an agent run driven by an inbound message. The agent runner is
    /// responsible for threading the `(plugin_id, channel, thread?)` tuple
    /// through so the agent's `message` tool can reply in-place.
    fn run_agent_from_event(
        &self,
        agent_id: &str,
        binding: &ChannelBinding,
        event: &TriggerEventPayload,
    );
}

pub struct Dispatcher {
    seen: Mutex<VecDeque<(String, Instant)>>,
    bindings: Arc<dyn DispatchBindings>,
}

impl Dispatcher {
    pub fn new(bindings: Arc<dyn DispatchBindings>) -> Self {
        Self {
            seen: Mutex::new(VecDeque::new()),
            bindings,
        }
    }

    pub async fn dispatch(&self, event: TriggerEventPayload) -> DispatchResult {
        if self.is_duplicate(&event.event_id) {
            debug!(event_id = %event.event_id, "trigger.emit: duplicate, dropping");
            return DispatchResult {
                duplicate: true,
                matched_workflows: 0,
                matched_agents: 0,
            };
        }

        // Agent fan-out: every matching binding spawns a fresh run. Apply the
        // `mentionOnly` filter here so the agent path stays in the dispatcher,
        // not leaked into the runner.
        let agent_matches = self
            .bindings
            .matching_agent_bindings(
                &event.plugin_id,
                &event.channel.id,
                event.channel.thread_id.as_deref(),
            )
            .await;
        let mut matched_agents = 0;
        for (agent_id, binding) in &agent_matches {
            if !binding.auto_respond {
                continue;
            }
            if binding.mention_only && !bot_is_mentioned(&event, &event.plugin_id) {
                continue;
            }
            self.bindings
                .run_agent_from_event(agent_id, binding, &event);
            matched_agents += 1;
        }

        // Workflow fan-out: the bindings impl is responsible for the
        // `triggerConfig` channel/thread match.
        let workflow_ids = self.bindings.matching_workflow_ids(&event).await;
        let matched_workflows = workflow_ids.len();
        for id in workflow_ids {
            self.bindings.run_workflow_from_event(&id, &event);
        }

        info!(
            event_id = %event.event_id,
            plugin_id = %event.plugin_id,
            kind = %event.kind,
            matched_workflows,
            matched_agents,
            "trigger.emit dispatched"
        );

        DispatchResult {
            duplicate: false,
            matched_workflows,
            matched_agents,
        }
    }

    fn is_duplicate(&self, event_id: &str) -> bool {
        let mut seen = self.seen.lock().expect("dispatcher seen poisoned");
        let now = Instant::now();
        // Evict expired entries from the front.
        while let Some((_, at)) = seen.front() {
            if now.duration_since(*at) > DEDUPE_TTL {
                seen.pop_front();
            } else {
                break;
            }
        }
        if seen.iter().any(|(id, _)| id == event_id) {
            return true;
        }
        if seen.len() >= DEDUPE_MAX_ENTRIES {
            seen.pop_front();
        }
        seen.push_back((event_id.to_string(), now));
        false
    }
}

fn bot_is_mentioned(event: &TriggerEventPayload, _plugin_id: &str) -> bool {
    // V1: the plugin is expected to populate `mentions` with the bot's user
    // id when applicable. If no list is provided, conservatively treat as
    // not-mentioned so `mentionOnly` bindings stay silent until the plugin
    // upgrades.
    !event.mentions.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct Spy {
        agent_matches: Vec<(String, ChannelBinding)>,
        workflow_ids: Vec<String>,
        agent_runs: StdMutex<Vec<String>>,
        workflow_runs: StdMutex<Vec<String>>,
    }

    #[async_trait]
    impl DispatchBindings for Spy {
        async fn matching_agent_bindings(
            &self,
            _plugin_id: &str,
            _channel_id: &str,
            _thread_id: Option<&str>,
        ) -> Vec<(String, ChannelBinding)> {
            self.agent_matches.clone()
        }

        async fn matching_workflow_ids(&self, _event: &TriggerEventPayload) -> Vec<String> {
            self.workflow_ids.clone()
        }

        fn run_workflow_from_event(&self, id: &str, _event: &TriggerEventPayload) {
            self.workflow_runs.lock().unwrap().push(id.to_string());
        }

        fn run_agent_from_event(
            &self,
            agent_id: &str,
            _binding: &ChannelBinding,
            _event: &TriggerEventPayload,
        ) {
            self.agent_runs.lock().unwrap().push(agent_id.to_string());
        }
    }

    fn event(event_id: &str) -> TriggerEventPayload {
        serde_json::from_value(json!({
            "eventId": event_id,
            "pluginId": "com.orbit.discord",
            "kind": "trigger.com_orbit_discord.message",
            "channel": { "id": "C1" },
            "user": { "id": "U1", "bot": false },
            "text": "hi",
            "mentions": [],
            "receivedAt": "2026-04-19T00:00:00Z"
        }))
        .unwrap()
    }

    fn binding(auto_respond: bool, mention_only: bool) -> ChannelBinding {
        ChannelBinding {
            id: "b1".into(),
            plugin_id: "com.orbit.discord".into(),
            provider_channel_id: "C1".into(),
            provider_thread_id: None,
            label: None,
            auto_respond,
            mention_only,
        }
    }

    #[tokio::test]
    async fn dedupe_drops_repeat_event_id() {
        let spy = Arc::new(Spy::default());
        let d = Dispatcher::new(spy.clone());
        assert!(!d.dispatch(event("m1")).await.duplicate);
        assert!(d.dispatch(event("m1")).await.duplicate);
        assert!(!d.dispatch(event("m2")).await.duplicate);
    }

    #[tokio::test]
    async fn agent_fan_out_respects_auto_respond_flag() {
        let mut spy = Spy::default();
        spy.agent_matches = vec![("agent-a".into(), binding(false, false))];
        let spy = Arc::new(spy);
        let d = Dispatcher::new(spy.clone());
        let r = d.dispatch(event("m1")).await;
        assert_eq!(r.matched_agents, 0);
        assert!(spy.agent_runs.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn mention_only_filters_unmentioned_events() {
        let mut spy = Spy::default();
        spy.agent_matches = vec![("agent-a".into(), binding(true, true))];
        let spy = Arc::new(spy);
        let d = Dispatcher::new(spy.clone());
        let r = d.dispatch(event("m1")).await;
        assert_eq!(r.matched_agents, 0);
    }

    #[tokio::test]
    async fn workflows_and_agents_fire_together() {
        let mut spy = Spy::default();
        spy.agent_matches = vec![("agent-a".into(), binding(true, false))];
        spy.workflow_ids = vec!["wf-1".into(), "wf-2".into()];
        let spy = Arc::new(spy);
        let d = Dispatcher::new(spy.clone());
        let r = d.dispatch(event("m1")).await;
        assert_eq!(r.matched_agents, 1);
        assert_eq!(r.matched_workflows, 2);
        assert_eq!(*spy.agent_runs.lock().unwrap(), vec!["agent-a"]);
        assert_eq!(
            *spy.workflow_runs.lock().unwrap(),
            vec!["wf-1".to_string(), "wf-2".to_string()]
        );
    }
}
