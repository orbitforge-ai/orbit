//! Production [`DispatchBindings`] implementation.
//!
//! Resolves which agents and workflows should fire for an inbound event by
//! reading persisted state through repos plus per-agent `config.json`
//! listen_bindings. Workflow matches start the workflow orchestrator; agent
//! matches use the desktop runtime hook.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::db::repos::{sqlite::SqliteRepos, Repos};
use crate::db::DbPool;
use crate::executor::workspace;
use crate::models::channel_binding::ChannelBinding;
use crate::models::trigger_event::TriggerEventPayload;
use crate::runtime_host::RuntimeHostHandle;
use crate::workflows::WorkflowOrchestrator;

use super::dispatcher::DispatchBindings;

pub struct ProductionBindings {
    db: DbPool,
    repos: Arc<dyn Repos>,
    host: RuntimeHostHandle,
}

impl ProductionBindings {
    pub fn new(db: DbPool, host: RuntimeHostHandle) -> Arc<Self> {
        let repos: Arc<dyn Repos> = Arc::new(SqliteRepos::new(db.clone()));
        Self::new_with_repos(db, repos, host)
    }

    pub fn new_with_repos(db: DbPool, repos: Arc<dyn Repos>, host: RuntimeHostHandle) -> Arc<Self> {
        Arc::new(Self { db, repos, host })
    }
}

#[async_trait]
impl DispatchBindings for ProductionBindings {
    async fn matching_agent_bindings(
        &self,
        plugin_id: &str,
        channel_id: &str,
        thread_id: Option<&str>,
    ) -> Vec<(String, ChannelBinding)> {
        let mut out = Vec::new();
        let Ok(agents) = self.repos.agents().list().await else {
            return out;
        };
        for agent in agents {
            let Ok(config) = workspace::load_agent_config(&agent.id) else {
                continue;
            };
            for binding in &config.listen_bindings {
                if binding.matches(plugin_id, channel_id, thread_id) {
                    out.push((agent.id.clone(), binding.clone()));
                }
            }
        }
        out
    }

    async fn matching_workflow_ids(&self, event: &TriggerEventPayload) -> Vec<String> {
        let Ok(workflows) = self.repos.project_workflows().list_enabled_triggers().await else {
            return Vec::new();
        };
        workflows
            .into_iter()
            .filter(|workflow| {
                if workflow.trigger_kind != event.kind {
                    return false;
                }
                let cfg = &workflow.trigger_config;
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
            .map(|workflow| workflow.id)
            .collect()
    }

    fn run_workflow_from_event(&self, workflow_id: &str, event: &TriggerEventPayload) {
        // Preserve the UI/event-log signal, then start the actual workflow
        // run on the runtime orchestrator.
        self.host.emit_json(
            "trigger:workflow",
            json!({
                "workflowId": workflow_id,
                "event": event,
            }),
        );
        let orchestrator = WorkflowOrchestrator::new_with_repos(
            self.db.clone(),
            self.repos.clone(),
            self.host.clone(),
        );
        let workflow_id = workflow_id.to_string();
        let workflow_id_for_task = workflow_id.clone();
        let trigger_kind = event.kind.clone();
        let trigger_kind_for_task = trigger_kind.clone();
        let trigger_data = serde_json::to_value(event).unwrap_or_else(|_| json!({}));
        tokio::spawn(async move {
            match orchestrator
                .start_run(
                    workflow_id_for_task.clone(),
                    &trigger_kind_for_task,
                    trigger_data,
                )
                .await
            {
                Ok(run) => tracing::info!(
                    run_id = run.id,
                    workflow_id = workflow_id_for_task,
                    trigger_kind = trigger_kind_for_task,
                    "trigger dispatch → workflow run started"
                ),
                Err(error) => tracing::warn!(
                    workflow_id = workflow_id_for_task,
                    trigger_kind = trigger_kind_for_task,
                    error = %error,
                    "trigger dispatch → workflow run failed to start"
                ),
            }
        });
        tracing::info!(
            workflow_id,
            event_id = %event.event_id,
            "trigger dispatch → workflow"
        );
    }

    fn run_agent_from_event(
        &self,
        agent_id: &str,
        binding: &ChannelBinding,
        event: &TriggerEventPayload,
    ) {
        // Inform the UI side (history, toasts, logs) that a trigger-driven
        // run is starting, then spawn the actual agent loop on a detached
        // task.
        self.host.emit_json(
            "trigger:agent",
            json!({
                "agentId": agent_id,
                "bindingId": binding.id,
                "event": event,
            }),
        );
        if let Some(app) = self.host.app_handle() {
            super::spawn::spawn_agent_run_from_event(
                app,
                agent_id.to_string(),
                binding.clone(),
                event.clone(),
            );
        } else {
            tracing::warn!(
                agent_id,
                event_id = %event.event_id,
                "trigger dispatch → agent skipped: no Tauri runtime host yet"
            );
        }
    }
}
