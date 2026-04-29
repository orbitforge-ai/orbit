//! Single-path DFS orchestrator for project workflows.
//!
//! v1 contract:
//! - Walks the graph from the unique trigger node, following exactly one
//!   outgoing edge per node.
//! - `logic.if` picks its `true` or `false` outgoing edge based on a recursive
//!   rule evaluator (see [`crate::workflows::rule_eval`]).
//! - Fan-in / join nodes are explicitly unsupported and are rejected at
//!   workflow save time, so the orchestrator can assume a single active path.
//! - A workflow run runs start-to-finish in one `tokio::spawn`. Crash
//!   recovery, parallel branches, and async wait states are out of scope.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::commands::project_workflows::workflow_has_trigger_node;
use crate::db::repos::{sqlite::SqliteRepos, Repos};
use crate::db::DbPool;
use crate::models::project_workflow::{WorkflowEdge, WorkflowGraph, WorkflowNode};
use crate::models::workflow_run::{WorkflowRun, WorkflowRunStep};
use crate::runtime_host::{emit_serialized, RuntimeHost, RuntimeHostHandle};
use crate::workflows::nodes::{self, NodeExecutionContext, NodeFailure, NodeOutcome};
use crate::workflows::store;
use crate::workflows::template::{build_reference_aliases, OUTPUT_ALIASES_KEY};

const MAX_STEPS: usize = 100;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowRunEventPayload {
    workflow_id: String,
    run_id: String,
    status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowRunStepEventPayload {
    workflow_id: String,
    run_id: String,
    step_id: String,
    node_id: String,
    node_type: String,
    status: String,
}

#[derive(Clone)]
pub struct WorkflowOrchestrator {
    db: DbPool,
    repos: Arc<dyn Repos>,
    host: RuntimeHostHandle,
}

impl WorkflowOrchestrator {
    pub fn new(db: DbPool, host: RuntimeHostHandle) -> Self {
        let repos: Arc<dyn Repos> = Arc::new(SqliteRepos::new(db.clone()));
        Self { db, repos, host }
    }

    pub fn new_with_repos(db: DbPool, repos: Arc<dyn Repos>, host: RuntimeHostHandle) -> Self {
        Self { db, repos, host }
    }

    /// Persist a `queued` `workflow_runs` row, then spawn the actual execution
    /// in the background. Returns the queued run immediately.
    pub async fn start_run(
        &self,
        workflow_id: String,
        trigger_kind: &str,
        trigger_data: Value,
    ) -> Result<WorkflowRun, String> {
        let workflow = self.repos.project_workflows().get(&workflow_id).await?;
        if !workflow_has_trigger_node(&workflow.graph) {
            return Err(serde_json::json!({
                "code": "workflow_missing_trigger",
                "message": "workflow cannot run without a trigger node",
            })
            .to_string());
        }
        let run = self
            .repos
            .workflow_runs()
            .create_run(&workflow, trigger_kind, &trigger_data)
            .await?;
        emit_run_event(
            self.host.as_ref(),
            &run.workflow_id,
            &run.id,
            &run.status,
            "workflow_run:created",
        );

        let this = WorkflowOrchestrator {
            db: self.db.clone(),
            repos: self.repos.clone(),
            host: self.host.clone(),
        };
        let run_clone = run.clone();
        let project_id = workflow.project_id.clone();
        let workflow_id = workflow.id.clone();
        tokio::spawn(async move {
            if let Err(e) = this.execute_run(run_clone, workflow_id, project_id).await {
                warn!("workflow run failed: {}", e);
            }
        });

        Ok(run)
    }

    async fn execute_run(
        &self,
        run: WorkflowRun,
        workflow_id: String,
        project_id: String,
    ) -> Result<(), String> {
        let started_at = Utc::now().to_rfc3339();
        self.update_run_status_event(
            &workflow_id,
            &run.id,
            store::STATUS_RUNNING,
            None,
            Some(&started_at),
            None,
        )
        .await
        .ok();

        let graph: WorkflowGraph = serde_json::from_value(run.graph_snapshot.clone())
            .map_err(|e| format!("invalid graph snapshot: {}", e))?;

        let by_id: HashMap<String, &WorkflowNode> =
            graph.nodes.iter().map(|n| (n.id.clone(), n)).collect();
        let outgoing: HashMap<String, Vec<&WorkflowEdge>> = group_edges(&graph.edges);

        let trigger = match graph
            .nodes
            .iter()
            .find(|n| n.node_type.starts_with("trigger."))
        {
            Some(t) => t,
            None => {
                let err = "no trigger node in workflow graph";
                self.fail_run_event(&workflow_id, &run.id, err).await.ok();
                return Err(err.into());
            }
        };

        let mut outputs: serde_json::Map<String, Value> = serde_json::Map::new();
        outputs.insert(
            "trigger".to_string(),
            json!({ "data": run.trigger_data, "kind": run.trigger_kind }),
        );
        outputs.insert(
            OUTPUT_ALIASES_KEY.to_string(),
            Value::Object(build_reference_aliases(&graph)),
        );

        let mut current = Some(trigger.id.clone());
        let mut sequence: i64 = 0;

        while let Some(node_id) = current {
            if sequence as usize >= MAX_STEPS {
                let err = format!("workflow exceeded {} steps; aborting", MAX_STEPS);
                self.fail_run_event(&workflow_id, &run.id, &err).await.ok();
                return Err(err);
            }

            let node = match by_id.get(&node_id) {
                Some(n) => *n,
                None => {
                    let err = format!("node {} referenced by edge not found", node_id);
                    self.fail_run_event(&workflow_id, &run.id, &err).await.ok();
                    return Err(err);
                }
            };

            let outputs_val = Value::Object(outputs.clone());
            let exec = self
                .execute_node(
                    &run.id,
                    &workflow_id,
                    &project_id,
                    node,
                    &outputs_val,
                    sequence,
                )
                .await;

            match exec {
                Ok(NodeOutcome {
                    output,
                    next_handle,
                }) => {
                    outputs.insert(node.id.clone(), json!({ "output": output }));
                    sequence += 1;
                    current = pick_next(&outgoing, &node.id, next_handle.as_deref());
                }
                Err(failure) => {
                    self.fail_run_event(&workflow_id, &run.id, &failure.message)
                        .await
                        .ok();
                    return Err(failure.message);
                }
            }
        }

        let completed_at = Utc::now().to_rfc3339();
        self.update_run_status_event(
            &workflow_id,
            &run.id,
            store::STATUS_SUCCESS,
            None,
            None,
            Some(&completed_at),
        )
        .await
        .ok();
        info!(run_id = run.id, steps = sequence, "workflow run completed");
        Ok(())
    }

    async fn update_run_status_event(
        &self,
        workflow_id: &str,
        run_id: &str,
        status: &str,
        error: Option<&str>,
        started_at: Option<&str>,
        completed_at: Option<&str>,
    ) -> Result<(), String> {
        self.repos
            .workflow_runs()
            .update_status(workflow_id, run_id, status, error, started_at, completed_at)
            .await?;
        emit_run_event(
            self.host.as_ref(),
            workflow_id,
            run_id,
            status,
            "workflow_run:updated",
        );
        Ok(())
    }

    async fn fail_run_event(
        &self,
        workflow_id: &str,
        run_id: &str,
        err: &str,
    ) -> Result<(), String> {
        let completed_at = Utc::now().to_rfc3339();
        self.update_run_status_event(
            workflow_id,
            run_id,
            store::STATUS_FAILED,
            Some(err),
            None,
            Some(&completed_at),
        )
        .await
    }

    async fn execute_node(
        &self,
        run_id: &str,
        workflow_id: &str,
        project_id: &str,
        node: &WorkflowNode,
        outputs: &Value,
        sequence: i64,
    ) -> Result<NodeOutcome, NodeFailure> {
        let started_at = Utc::now().to_rfc3339();
        let input = json!({ "node_data": node.data, "upstream": outputs });

        let step = self
            .repos
            .workflow_runs()
            .insert_step(
                run_id,
                &node.id,
                &node.node_type,
                store::STATUS_RUNNING,
                &input,
                Some(&started_at),
                sequence,
            )
            .await?;
        emit_step_event(
            self.host.as_ref(),
            workflow_id,
            run_id,
            &step.id,
            &node.id,
            &node.node_type,
            store::STATUS_RUNNING,
        );

        let result = nodes::execute(NodeExecutionContext {
            db: &self.db,
            repos: self.repos.as_ref(),
            host: self.host.as_ref(),
            run_id,
            workflow_id,
            project_id,
            node,
            outputs,
        })
        .await;

        let completed_at = Utc::now().to_rfc3339();
        match &result {
            Ok(outcome) => {
                self.repos
                    .workflow_runs()
                    .finish_step(
                        run_id,
                        &step.id,
                        store::STATUS_SUCCESS,
                        Some(&outcome.output),
                        None,
                        &completed_at,
                    )
                    .await?;
                emit_step_event(
                    self.host.as_ref(),
                    workflow_id,
                    run_id,
                    &step.id,
                    &node.id,
                    &node.node_type,
                    store::STATUS_SUCCESS,
                );
            }
            Err(failure) => {
                self.repos
                    .workflow_runs()
                    .finish_step(
                        run_id,
                        &step.id,
                        store::STATUS_FAILED,
                        failure.partial_output.as_ref(),
                        Some(&failure.message),
                        &completed_at,
                    )
                    .await?;
                emit_step_event(
                    self.host.as_ref(),
                    workflow_id,
                    run_id,
                    &step.id,
                    &node.id,
                    &node.node_type,
                    store::STATUS_FAILED,
                );
            }
        }
        result
    }
}

fn emit_run_event(
    host: &dyn RuntimeHost,
    workflow_id: &str,
    run_id: &str,
    status: &str,
    event: &'static str,
) {
    emit_serialized(
        host,
        event,
        &WorkflowRunEventPayload {
            workflow_id: workflow_id.to_string(),
            run_id: run_id.to_string(),
            status: status.to_string(),
        },
    );
}

fn emit_step_event(
    host: &dyn RuntimeHost,
    workflow_id: &str,
    run_id: &str,
    step_id: &str,
    node_id: &str,
    node_type: &str,
    status: &str,
) {
    emit_serialized(
        host,
        "workflow_run:step",
        &WorkflowRunStepEventPayload {
            workflow_id: workflow_id.to_string(),
            run_id: run_id.to_string(),
            step_id: step_id.to_string(),
            node_id: node_id.to_string(),
            node_type: node_type.to_string(),
            status: status.to_string(),
        },
    );
}

fn group_edges(edges: &[WorkflowEdge]) -> HashMap<String, Vec<&WorkflowEdge>> {
    let mut map: HashMap<String, Vec<&WorkflowEdge>> = HashMap::new();
    for edge in edges {
        map.entry(edge.source.clone()).or_default().push(edge);
    }
    map
}

fn pick_next(
    outgoing: &HashMap<String, Vec<&WorkflowEdge>>,
    source_id: &str,
    handle: Option<&str>,
) -> Option<String> {
    let edges = outgoing.get(source_id)?;
    if let Some(h) = handle {
        for edge in edges {
            if edge.source_handle.as_deref() == Some(h) {
                return Some(edge.target.clone());
            }
        }
        None
    } else {
        edges.first().map(|edge| edge.target.clone())
    }
}

pub fn load_run_with_steps(
    pool: &DbPool,
    run_id: &str,
) -> Result<(WorkflowRun, Vec<WorkflowRunStep>), String> {
    store::load_run_with_steps(pool, run_id)
}

pub fn list_runs_for_workflow(
    pool: &DbPool,
    workflow_id: &str,
    limit: i64,
) -> Result<Vec<WorkflowRun>, String> {
    store::list_runs_for_workflow(pool, workflow_id, limit)
}

pub fn cancel_run(pool: &DbPool, run_id: &str) -> Result<(), String> {
    store::cancel_run(pool, run_id)
}

#[cfg(test)]
mod tests {
    use super::{group_edges, pick_next, WorkflowOrchestrator};
    use crate::db::connection::init as init_db;
    use crate::models::project_workflow::ProjectWorkflow;
    use crate::models::project_workflow::{
        NodePosition, WorkflowEdge, WorkflowGraph, WorkflowNode,
    };
    use crate::workflows::orchestrator::load_run_with_steps;
    use crate::workflows::store;
    use serde_json::{json, Value};
    use std::path::PathBuf;

    fn temp_db_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("orbit-workflow-{}-{}", name, ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn trigger_node() -> WorkflowNode {
        WorkflowNode {
            id: "trigger-1".into(),
            node_type: "trigger.manual".into(),
            position: NodePosition { x: 0.0, y: 0.0 },
            data: json!({}),
        }
    }

    fn workflow(id: &str, project_id: &str, graph: WorkflowGraph) -> ProjectWorkflow {
        ProjectWorkflow {
            id: id.into(),
            project_id: project_id.into(),
            name: "Workflow".into(),
            description: None,
            enabled: true,
            graph,
            trigger_kind: "manual".into(),
            trigger_config: Value::Null,
            version: 1,
            created_at: "2024-01-01T00:00:00Z".into(),
            updated_at: "2024-01-01T00:00:00Z".into(),
        }
    }

    fn seed_workflow_fixture(db: &crate::db::DbPool, workflow: &ProjectWorkflow) {
        let conn = db.get().unwrap();
        let graph = serde_json::to_string(&workflow.graph).unwrap();
        let trigger_config = workflow.trigger_config.to_string();
        conn.execute(
            "INSERT INTO projects (id, name, description, created_at, updated_at, tenant_id)
             VALUES (?1, ?2, NULL, ?3, ?3, 'local')",
            rusqlite::params![workflow.project_id, "Test Project", workflow.created_at],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project_workflows
                (id, project_id, name, description, enabled, graph, trigger_kind, trigger_config, version, created_at, updated_at, tenant_id)
             VALUES
                (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, COALESCE((SELECT tenant_id FROM projects WHERE id = ?2), 'local'))",
            rusqlite::params![
                workflow.id,
                workflow.project_id,
                workflow.name,
                workflow.description,
                workflow.enabled,
                graph,
                workflow.trigger_kind,
                trigger_config,
                workflow.version,
                workflow.created_at,
                workflow.updated_at,
            ],
        )
        .unwrap();
    }

    #[test]
    fn pick_next_uses_handle_for_logic_if() {
        let edges = vec![
            WorkflowEdge {
                id: "e1".into(),
                source: "if1".into(),
                target: "yes".into(),
                source_handle: Some("true".into()),
                target_handle: None,
            },
            WorkflowEdge {
                id: "e2".into(),
                source: "if1".into(),
                target: "no".into(),
                source_handle: Some("false".into()),
                target_handle: None,
            },
        ];
        let outgoing = group_edges(&edges);
        assert_eq!(
            pick_next(&outgoing, "if1", Some("true")),
            Some("yes".into())
        );
        assert_eq!(
            pick_next(&outgoing, "if1", Some("false")),
            Some("no".into())
        );
        assert_eq!(pick_next(&outgoing, "if1", Some("other")), None);
    }

    #[tokio::test]
    async fn execute_run_completes_simple_trigger_workflow() {
        let dir = temp_db_dir("happy");
        let db = init_db(dir.clone()).unwrap();
        let host = crate::runtime_host::headless_host();
        let orchestrator = WorkflowOrchestrator::new(db.clone(), host.clone());
        let wf = workflow(
            "wf-happy",
            "project-1",
            WorkflowGraph {
                nodes: vec![trigger_node()],
                edges: Vec::new(),
                schema_version: 1,
            },
        );
        seed_workflow_fixture(&db, &wf);
        let run = orchestrator
            .repos
            .workflow_runs()
            .create_run(&wf, "manual", &json!({"ok": true}))
            .await
            .unwrap();

        orchestrator
            .execute_run(run.clone(), wf.id.clone(), wf.project_id.clone())
            .await
            .unwrap();

        let (stored_run, steps) = load_run_with_steps(&db, &run.id).unwrap();
        assert_eq!(stored_run.status, store::STATUS_SUCCESS);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].status, store::STATUS_SUCCESS);
        assert_eq!(steps[0].node_type, "trigger.manual");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn execute_run_marks_failed_node_and_run() {
        let dir = temp_db_dir("failure");
        let db = init_db(dir.clone()).unwrap();
        let host = crate::runtime_host::headless_host();
        let orchestrator = WorkflowOrchestrator::new(db.clone(), host.clone());
        let wf = workflow(
            "wf-failure",
            "project-1",
            WorkflowGraph {
                nodes: vec![
                    trigger_node(),
                    WorkflowNode {
                        id: "bad-1".into(),
                        node_type: "integration.unknown".into(),
                        position: NodePosition { x: 1.0, y: 0.0 },
                        data: json!({}),
                    },
                ],
                edges: vec![WorkflowEdge {
                    id: "edge-1".into(),
                    source: "trigger-1".into(),
                    target: "bad-1".into(),
                    source_handle: None,
                    target_handle: None,
                }],
                schema_version: 1,
            },
        );
        seed_workflow_fixture(&db, &wf);
        let run = orchestrator
            .repos
            .workflow_runs()
            .create_run(&wf, "manual", &Value::Null)
            .await
            .unwrap();

        let err = orchestrator
            .execute_run(run.clone(), wf.id.clone(), wf.project_id.clone())
            .await
            .unwrap_err();

        assert!(err.contains("unknown node type"));

        let (stored_run, steps) = load_run_with_steps(&db, &run.id).unwrap();
        assert_eq!(stored_run.status, store::STATUS_FAILED);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].status, store::STATUS_SUCCESS);
        assert_eq!(steps[1].status, store::STATUS_FAILED);
        assert_eq!(steps[1].node_id, "bad-1");

        let _ = std::fs::remove_dir_all(dir);
    }
}
