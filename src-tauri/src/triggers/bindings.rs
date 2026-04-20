//! Production [`DispatchBindings`] implementation.
//!
//! Resolves which agents and workflows should fire for an inbound event by
//! reading persisted state (sqlite for workflows, per-agent `config.json` for
//! listen_bindings). The actual *execution* side — spawning workflow runs and
//! agent runs — is represented here as emitted Tauri events so the wider
//! backend can bind a concrete executor without coupling the dispatcher to
//! it. Phase 1 ships the event emission; the orchestrator and agent-runner
//! hooks land in follow-up slices.

use std::sync::Arc;

use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::db::DbPool;
use crate::executor::workspace;
use crate::models::channel_binding::ChannelBinding;
use crate::models::trigger_event::TriggerEventPayload;

use super::dispatcher::DispatchBindings;

pub struct ProductionBindings {
    db: DbPool,
    app: AppHandle,
}

impl ProductionBindings {
    pub fn new(db: DbPool, app: AppHandle) -> Arc<Self> {
        Arc::new(Self { db, app })
    }

    fn agent_ids(&self) -> Vec<String> {
        let Ok(conn) = self.db.0.get() else {
            return Vec::new();
        };
        let Ok(mut stmt) = conn.prepare("SELECT id FROM agents") else {
            return Vec::new();
        };
        stmt.query_map([], |row| row.get::<_, String>(0))
            .ok()
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }
}

impl DispatchBindings for ProductionBindings {
    fn matching_agent_bindings(
        &self,
        plugin_id: &str,
        channel_id: &str,
        thread_id: Option<&str>,
    ) -> Vec<(String, ChannelBinding)> {
        let mut out = Vec::new();
        for agent_id in self.agent_ids() {
            let Ok(config) = workspace::load_agent_config(&agent_id) else {
                continue;
            };
            for binding in &config.listen_bindings {
                if binding.matches(plugin_id, channel_id, thread_id) {
                    out.push((agent_id.clone(), binding.clone()));
                }
            }
        }
        out
    }

    fn matching_workflow_ids(&self, event: &TriggerEventPayload) -> Vec<String> {
        let Ok(conn) = self.db.0.get() else {
            return Vec::new();
        };
        let Ok(mut stmt) = conn.prepare(
            "SELECT id, trigger_config FROM project_workflows \
             WHERE enabled = 1 AND trigger_kind = ?1",
        ) else {
            return Vec::new();
        };
        let rows = stmt
            .query_map(rusqlite::params![event.kind], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .ok();
        let Some(rows) = rows else {
            return Vec::new();
        };

        rows.filter_map(Result::ok)
            .filter(|(_, cfg_json)| {
                let cfg: serde_json::Value =
                    serde_json::from_str(cfg_json).unwrap_or_else(|_| json!({}));
                let ch_id = cfg.get("channelId").and_then(|v| v.as_str());
                let t_id = cfg.get("threadId").and_then(|v| v.as_str());
                match ch_id {
                    // A workflow with no channelId configured is treated as
                    // "match any" — useful for fan-out patterns.
                    None => true,
                    Some(expected) if expected == event.channel.id => match t_id {
                        None => true,
                        Some(expected_thread) => {
                            Some(expected_thread) == event.channel.thread_id.as_deref()
                        }
                    },
                    Some(_) => false,
                }
            })
            .map(|(id, _)| id)
            .collect()
    }

    fn run_workflow_from_event(&self, workflow_id: &str, event: &TriggerEventPayload) {
        // Phase 1: surface as a Tauri event. The workflow orchestrator
        // integration (`run_from_trigger_event`) lands in the next slice.
        let _ = self.app.emit(
            "trigger:workflow",
            json!({
                "workflowId": workflow_id,
                "event": event,
            }),
        );
        tracing::info!(
            workflow_id,
            event_id = %event.event_id,
            "trigger dispatch → workflow (awaiting orchestrator wiring)"
        );
    }

    fn run_agent_from_event(
        &self,
        agent_id: &str,
        binding: &ChannelBinding,
        event: &TriggerEventPayload,
    ) {
        // Phase 1: surface as a Tauri event. The agent-runner integration
        // (spawning a new run seeded with the inbound message and reply
        // context) lands in the next slice.
        let _ = self.app.emit(
            "trigger:agent",
            json!({
                "agentId": agent_id,
                "bindingId": binding.id,
                "event": event,
            }),
        );
        tracing::info!(
            agent_id,
            binding_id = %binding.id,
            event_id = %event.event_id,
            "trigger dispatch → agent (awaiting runner wiring)"
        );
    }
}
