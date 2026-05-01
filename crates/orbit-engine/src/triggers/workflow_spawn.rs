use std::sync::Arc;

use serde_json::json;

use crate::db::repos::Repos;
use crate::db::DbPool;
use crate::models::trigger_event::TriggerEventPayload;
use crate::runtime_host::RuntimeHostHandle;
use crate::workflows::WorkflowOrchestrator;

pub(crate) fn spawn_workflow_run(
    db: DbPool,
    repos: Arc<dyn Repos>,
    host: RuntimeHostHandle,
    workflow_id: String,
    event: &TriggerEventPayload,
) {
    // Preserve the UI/event-log signal, then start the actual workflow run on
    // the runtime orchestrator.
    host.emit_json(
        "trigger:workflow",
        json!({
            "workflowId": workflow_id,
            "event": event,
        }),
    );

    let orchestrator = WorkflowOrchestrator::new_with_repos(db, repos, host);
    let workflow_id_for_task = workflow_id.clone();
    let trigger_kind = event.kind.clone();
    let trigger_kind_for_task = trigger_kind.clone();
    let event_id = event.event_id.clone();
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
    tracing::info!(workflow_id, event_id, "trigger dispatch → workflow");
}
