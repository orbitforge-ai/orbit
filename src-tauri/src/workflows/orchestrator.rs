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

use chrono::Utc;
use serde_json::{json, Value};
use tauri::Emitter;
use tracing::{info, warn};
use ulid::Ulid;

use crate::db::DbPool;
use crate::executor::{keychain, llm_provider, workspace};
use crate::models::project_workflow::{
    ProjectWorkflow, RuleNode, WorkflowEdge, WorkflowGraph, WorkflowNode,
};
use crate::models::workflow_run::{WorkflowRun, WorkflowRunStep};
use crate::workflows::rule_eval::eval_rule;

const STATUS_QUEUED: &str = "queued";
const STATUS_RUNNING: &str = "running";
const STATUS_SUCCESS: &str = "success";
const STATUS_FAILED: &str = "failed";
const STATUS_SKIPPED: &str = "skipped";

const MAX_STEPS: usize = 100;

#[derive(Clone)]
pub struct WorkflowOrchestrator {
    db: DbPool,
    app: tauri::AppHandle,
}

impl WorkflowOrchestrator {
    pub fn new(db: DbPool, app: tauri::AppHandle) -> Self {
        Self { db, app }
    }

    /// Persist a `queued` `workflow_runs` row, then spawn the actual execution
    /// in the background. Returns the queued run immediately.
    pub async fn start_run(
        &self,
        workflow_id: String,
        trigger_kind: &str,
        trigger_data: Value,
    ) -> Result<WorkflowRun, String> {
        let workflow = self.load_workflow(&workflow_id).await?;
        let run = self
            .insert_run(&workflow, trigger_kind, &trigger_data)
            .await?;

        let this = self.clone();
        let run_clone = run.clone();
        tokio::spawn(async move {
            if let Err(e) = this.execute_run(run_clone).await {
                warn!("workflow run failed: {}", e);
            }
        });

        Ok(run)
    }

    async fn load_workflow(&self, workflow_id: &str) -> Result<ProjectWorkflow, String> {
        let pool = self.db.0.clone();
        let id = workflow_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<ProjectWorkflow, String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            conn.query_row(
                "SELECT id, project_id, name, description, enabled, graph, trigger_kind,
                        trigger_config, version, created_at, updated_at
                 FROM project_workflows WHERE id = ?1",
                rusqlite::params![id],
                |row| {
                    let graph_str: String = row.get(5)?;
                    let trigger_cfg_str: String = row.get(7)?;
                    let graph: WorkflowGraph =
                        serde_json::from_str(&graph_str).unwrap_or_default();
                    let trigger_config: Value =
                        serde_json::from_str(&trigger_cfg_str).unwrap_or(Value::Null);
                    Ok(ProjectWorkflow {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        name: row.get(2)?,
                        description: row.get(3)?,
                        enabled: row.get::<_, bool>(4)?,
                        graph,
                        trigger_kind: row.get(6)?,
                        trigger_config,
                        version: row.get(8)?,
                        created_at: row.get(9)?,
                        updated_at: row.get(10)?,
                    })
                },
            )
            .map_err(|e| format!("workflow {} not found: {}", id, e))
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn insert_run(
        &self,
        workflow: &ProjectWorkflow,
        trigger_kind: &str,
        trigger_data: &Value,
    ) -> Result<WorkflowRun, String> {
        let pool = self.db.0.clone();
        let id = Ulid::new().to_string();
        let now = Utc::now().to_rfc3339();
        let workflow_id = workflow.id.clone();
        let workflow_version = workflow.version;
        let graph_str = serde_json::to_string(&workflow.graph).unwrap_or_else(|_| "{}".into());
        let trigger_kind = trigger_kind.to_string();
        let trigger_data_str = serde_json::to_string(trigger_data).unwrap_or_else(|_| "{}".into());

        let id_clone = id.clone();
        let now_clone = now.clone();
        let trigger_kind_clone = trigger_kind.clone();
        let trigger_data_str_clone = trigger_data_str.clone();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT INTO workflow_runs (id, workflow_id, workflow_version, graph_snapshot,
                                            trigger_kind, trigger_data, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    id_clone,
                    workflow_id,
                    workflow_version,
                    graph_str,
                    trigger_kind_clone,
                    trigger_data_str_clone,
                    STATUS_QUEUED,
                    now_clone,
                ],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())??;

        let run = WorkflowRun {
            id,
            workflow_id: workflow.id.clone(),
            workflow_version,
            graph_snapshot: serde_json::to_value(&workflow.graph).unwrap_or(Value::Null),
            trigger_kind,
            trigger_data: trigger_data.clone(),
            status: STATUS_QUEUED.to_string(),
            error: None,
            started_at: None,
            completed_at: None,
            created_at: now,
        };
        let _ = self.app.emit("workflow_run:created", &run);
        Ok(run)
    }

    async fn execute_run(&self, run: WorkflowRun) -> Result<(), String> {
        let started_at = Utc::now().to_rfc3339();
        self.update_run_status(&run.id, STATUS_RUNNING, None, Some(&started_at), None)
            .await
            .ok();

        let graph: WorkflowGraph = serde_json::from_value(run.graph_snapshot.clone())
            .map_err(|e| format!("invalid graph snapshot: {}", e))?;

        let by_id: HashMap<String, &WorkflowNode> =
            graph.nodes.iter().map(|n| (n.id.clone(), n)).collect();
        let outgoing: HashMap<String, Vec<&WorkflowEdge>> = group_edges(&graph.edges);

        let trigger = match graph.nodes.iter().find(|n| n.node_type.starts_with("trigger.")) {
            Some(t) => t,
            None => {
                let err = "no trigger node in workflow graph";
                self.fail_run(&run.id, err).await.ok();
                return Err(err.into());
            }
        };

        let mut outputs: serde_json::Map<String, Value> = serde_json::Map::new();
        outputs.insert(
            "trigger".to_string(),
            json!({ "data": run.trigger_data, "kind": run.trigger_kind }),
        );

        let mut current = Some(trigger.id.clone());
        let mut sequence: i64 = 0;

        while let Some(node_id) = current {
            if sequence as usize >= MAX_STEPS {
                let err = format!("workflow exceeded {} steps; aborting", MAX_STEPS);
                self.fail_run(&run.id, &err).await.ok();
                return Err(err);
            }

            let node = match by_id.get(&node_id) {
                Some(n) => *n,
                None => {
                    let err = format!("node {} referenced by edge not found", node_id);
                    self.fail_run(&run.id, &err).await.ok();
                    return Err(err);
                }
            };

            let outputs_val = Value::Object(outputs.clone());
            let exec = self
                .execute_node(&run.id, node, &outputs_val, sequence)
                .await;

            match exec {
                Ok(NodeOutcome { output, next_handle }) => {
                    outputs.insert(node.id.clone(), json!({ "output": output }));
                    sequence += 1;
                    current = pick_next(&outgoing, &node.id, next_handle.as_deref());
                }
                Err(err_msg) => {
                    self.fail_run(&run.id, &err_msg).await.ok();
                    return Err(err_msg);
                }
            }
        }

        let completed_at = Utc::now().to_rfc3339();
        self.update_run_status(
            &run.id,
            STATUS_SUCCESS,
            None,
            None,
            Some(&completed_at),
        )
        .await
        .ok();
        info!(run_id = run.id, steps = sequence, "workflow run completed");
        Ok(())
    }

    async fn execute_node(
        &self,
        run_id: &str,
        node: &WorkflowNode,
        outputs: &Value,
        sequence: i64,
    ) -> Result<NodeOutcome, String> {
        let step_id = Ulid::new().to_string();
        let started_at = Utc::now().to_rfc3339();
        let input = json!({ "node_data": node.data, "upstream": outputs });

        self.insert_step(
            &step_id,
            run_id,
            &node.id,
            &node.node_type,
            STATUS_RUNNING,
            &input,
            Some(&started_at),
            sequence,
        )
        .await?;

        let result = match node.node_type.as_str() {
            "trigger.manual" | "trigger.schedule" => Ok(NodeOutcome {
                output: outputs
                    .get("trigger")
                    .cloned()
                    .unwrap_or(Value::Null),
                next_handle: None,
            }),
            "agent.run" => self.run_agent_node(node, outputs).await,
            "logic.if" => self.run_logic_if(node, outputs).await,
            other if other.starts_with("integration.") => Err(format!(
                "integration node `{}` is not yet implemented",
                other
            )),
            other => Err(format!("unknown node type `{}`", other)),
        };

        let completed_at = Utc::now().to_rfc3339();
        match &result {
            Ok(outcome) => {
                self.finish_step(
                    &step_id,
                    STATUS_SUCCESS,
                    Some(&outcome.output),
                    None,
                    &completed_at,
                )
                .await?;
            }
            Err(err) => {
                self.finish_step(&step_id, STATUS_FAILED, None, Some(err), &completed_at)
                    .await?;
            }
        }
        result
    }

    // ── Node executors ──────────────────────────────────────────────────────

    async fn run_agent_node(
        &self,
        node: &WorkflowNode,
        outputs: &Value,
    ) -> Result<NodeOutcome, String> {
        let agent_id = node
            .data
            .get("agentId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "agent.run requires data.agentId".to_string())?
            .to_string();
        let template = node
            .data
            .get("promptTemplate")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let prompt = render_template(&template, outputs);

        let ws_config = workspace::load_agent_config(&agent_id).unwrap_or_default();
        if ws_config.provider.is_empty() {
            return Err(format!("agent {} has no provider configured", agent_id));
        }
        let api_key = keychain::retrieve_api_key(&ws_config.provider).map_err(|_| {
            format!(
                "no API key configured for provider `{}`",
                ws_config.provider
            )
        })?;
        let provider = llm_provider::create_provider(&ws_config.provider, api_key)?;

        let llm_config = llm_provider::LlmConfig {
            model: ws_config.model.clone(),
            max_tokens: 4_096,
            temperature: Some(ws_config.temperature),
            system_prompt: ws_config
                .role_system_instructions
                .clone()
                .unwrap_or_default(),
        };
        let messages = vec![llm_provider::ChatMessage {
            role: "user".to_string(),
            content: vec![llm_provider::ContentBlock::Text {
                text: prompt.clone(),
            }],
            created_at: None,
        }];

        let response = provider
            .chat_complete(&llm_config, &messages, &[])
            .await
            .map_err(|e| format!("agent.run LLM call failed: {}", e))?;

        let text = llm_provider::extract_text_response(&response).unwrap_or_default();
        let parsed: Value =
            serde_json::from_str(&text).unwrap_or_else(|_| Value::String(text.clone()));

        Ok(NodeOutcome {
            output: json!({
                "agentId": agent_id,
                "prompt": prompt,
                "text": text,
                "parsed": parsed,
            }),
            next_handle: None,
        })
    }

    async fn run_logic_if(
        &self,
        node: &WorkflowNode,
        outputs: &Value,
    ) -> Result<NodeOutcome, String> {
        let rule_value = node
            .data
            .get("rule")
            .ok_or_else(|| "logic.if requires data.rule".to_string())?;
        let rule: RuleNode = serde_json::from_value(rule_value.clone())
            .map_err(|e| format!("invalid logic.if rule: {}", e))?;

        let result = eval_rule(&rule, outputs);
        let handle = if result { "true" } else { "false" };
        Ok(NodeOutcome {
            output: json!({ "result": result, "branch": handle }),
            next_handle: Some(handle.to_string()),
        })
    }

    // ── Persistence helpers ─────────────────────────────────────────────────

    async fn update_run_status(
        &self,
        run_id: &str,
        status: &str,
        error: Option<&str>,
        started_at: Option<&str>,
        completed_at: Option<&str>,
    ) -> Result<(), String> {
        let pool = self.db.0.clone();
        let run_id = run_id.to_string();
        let status = status.to_string();
        let error = error.map(String::from);
        let started_at = started_at.map(String::from);
        let completed_at = completed_at.map(String::from);
        let app = self.app.clone();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE workflow_runs SET status = ?1, error = COALESCE(?2, error),
                                          started_at = COALESCE(?3, started_at),
                                          completed_at = COALESCE(?4, completed_at)
                 WHERE id = ?5",
                rusqlite::params![status, error, started_at, completed_at, run_id],
            )
            .map_err(|e| e.to_string())?;
            let _ = app.emit("workflow_run:updated", &run_id);
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn fail_run(&self, run_id: &str, err: &str) -> Result<(), String> {
        let now = Utc::now().to_rfc3339();
        self.update_run_status(run_id, STATUS_FAILED, Some(err), None, Some(&now))
            .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn insert_step(
        &self,
        step_id: &str,
        run_id: &str,
        node_id: &str,
        node_type: &str,
        status: &str,
        input: &Value,
        started_at: Option<&str>,
        sequence: i64,
    ) -> Result<(), String> {
        let pool = self.db.0.clone();
        let step_id = step_id.to_string();
        let run_id = run_id.to_string();
        let node_id = node_id.to_string();
        let node_type = node_type.to_string();
        let status = status.to_string();
        let input_str = serde_json::to_string(input).unwrap_or_else(|_| "{}".into());
        let started_at = started_at.map(String::from);
        let app = self.app.clone();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT INTO workflow_run_steps (id, run_id, node_id, node_type, status, input,
                                                  started_at, sequence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    step_id,
                    run_id,
                    node_id,
                    node_type,
                    status,
                    input_str,
                    started_at,
                    sequence,
                ],
            )
            .map_err(|e| e.to_string())?;
            let _ = app.emit("workflow_run:step", &run_id);
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn finish_step(
        &self,
        step_id: &str,
        status: &str,
        output: Option<&Value>,
        error: Option<&str>,
        completed_at: &str,
    ) -> Result<(), String> {
        let pool = self.db.0.clone();
        let step_id = step_id.to_string();
        let status = status.to_string();
        let output_str = output.map(|v| serde_json::to_string(v).unwrap_or_else(|_| "{}".into()));
        let error = error.map(String::from);
        let completed_at = completed_at.to_string();
        let app = self.app.clone();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE workflow_run_steps SET status = ?1, output = ?2, error = ?3,
                                                completed_at = ?4
                 WHERE id = ?5",
                rusqlite::params![status, output_str, error, completed_at, step_id],
            )
            .map_err(|e| e.to_string())?;
            let _ = app.emit("workflow_run:step", &step_id);
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

struct NodeOutcome {
    output: Value,
    next_handle: Option<String>,
}

fn group_edges(edges: &[WorkflowEdge]) -> HashMap<String, Vec<&WorkflowEdge>> {
    let mut map: HashMap<String, Vec<&WorkflowEdge>> = HashMap::new();
    for e in edges {
        map.entry(e.source.clone()).or_default().push(e);
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
        for e in edges {
            if e.source_handle.as_deref() == Some(h) {
                return Some(e.target.clone());
            }
        }
        // No matching handle: this branch dead-ends.
        None
    } else {
        edges.first().map(|e| e.target.clone())
    }
}

/// Tiny mustache-style `{{path.to.value}}` renderer for prompt templates.
/// Resolves against the `outputs` map produced by upstream nodes.
fn render_template(template: &str, outputs: &Value) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if let Some(end) = find_close(&bytes[i + 2..]) {
                let path = template[i + 2..i + 2 + end].trim();
                let value = lookup_path(path, outputs);
                out.push_str(&value);
                i += 2 + end + 2;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn find_close(bytes: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'}' && bytes[i + 1] == b'}' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn lookup_path(path: &str, outputs: &Value) -> String {
    let mut cur = outputs;
    for segment in path.split('.') {
        match cur.get(segment) {
            Some(v) => cur = v,
            None => return format!("{{{{{}}}}}", path),
        }
    }
    match cur {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

// ── Read helpers used by commands ───────────────────────────────────────────

pub fn load_run_with_steps(
    pool: &DbPool,
    run_id: &str,
) -> Result<(WorkflowRun, Vec<WorkflowRunStep>), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let run = conn
        .query_row(
            "SELECT id, workflow_id, workflow_version, graph_snapshot, trigger_kind,
                    trigger_data, status, error, started_at, completed_at, created_at
             FROM workflow_runs WHERE id = ?1",
            rusqlite::params![run_id],
            map_run_row,
        )
        .map_err(|e| format!("workflow run {} not found: {}", run_id, e))?;

    let mut stmt = conn
        .prepare(
            "SELECT id, run_id, node_id, node_type, status, input, output, error,
                    started_at, completed_at, sequence
             FROM workflow_run_steps WHERE run_id = ?1 ORDER BY sequence ASC",
        )
        .map_err(|e| e.to_string())?;
    let steps = stmt
        .query_map(rusqlite::params![run_id], map_step_row)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok((run, steps))
}

pub fn list_runs_for_workflow(
    pool: &DbPool,
    workflow_id: &str,
    limit: i64,
) -> Result<Vec<WorkflowRun>, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, workflow_id, workflow_version, graph_snapshot, trigger_kind,
                    trigger_data, status, error, started_at, completed_at, created_at
             FROM workflow_runs WHERE workflow_id = ?1
             ORDER BY created_at DESC LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![workflow_id, limit], map_run_row)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn cancel_run(pool: &DbPool, run_id: &str) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE workflow_runs
         SET status = ?1, error = COALESCE(error, 'cancelled'), completed_at = ?2
         WHERE id = ?3 AND status IN ('queued', 'running')",
        rusqlite::params![STATUS_FAILED, now, run_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE workflow_run_steps
         SET status = ?1, error = COALESCE(error, 'cancelled'), completed_at = ?2
         WHERE run_id = ?3 AND status IN ('queued', 'running')",
        rusqlite::params![STATUS_SKIPPED, now, run_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn map_run_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkflowRun> {
    let graph_str: String = row.get(3)?;
    let trigger_str: String = row.get(5)?;
    Ok(WorkflowRun {
        id: row.get(0)?,
        workflow_id: row.get(1)?,
        workflow_version: row.get(2)?,
        graph_snapshot: serde_json::from_str(&graph_str).unwrap_or(Value::Null),
        trigger_kind: row.get(4)?,
        trigger_data: serde_json::from_str(&trigger_str).unwrap_or(Value::Null),
        status: row.get(6)?,
        error: row.get(7)?,
        started_at: row.get(8)?,
        completed_at: row.get(9)?,
        created_at: row.get(10)?,
    })
}

fn map_step_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkflowRunStep> {
    let input_str: String = row.get(5)?;
    let output_opt: Option<String> = row.get(6)?;
    Ok(WorkflowRunStep {
        id: row.get(0)?,
        run_id: row.get(1)?,
        node_id: row.get(2)?,
        node_type: row.get(3)?,
        status: row.get(4)?,
        input: serde_json::from_str(&input_str).unwrap_or(Value::Null),
        output: output_opt.and_then(|s| serde_json::from_str(&s).ok()),
        error: row.get(7)?,
        started_at: row.get(8)?,
        completed_at: row.get(9)?,
        sequence: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::{render_template, pick_next, group_edges};
    use crate::models::project_workflow::WorkflowEdge;
    use serde_json::json;

    #[test]
    fn template_renders_known_paths() {
        let outputs = json!({
            "trigger": { "data": { "subject": "Hi" } }
        });
        assert_eq!(
            render_template("Subject: {{trigger.data.subject}}", &outputs),
            "Subject: Hi"
        );
    }

    #[test]
    fn template_leaves_unknown_paths_intact() {
        let outputs = json!({});
        assert_eq!(
            render_template("Hello {{missing.path}}!", &outputs),
            "Hello {{missing.path}}!"
        );
    }

    #[test]
    fn pick_next_uses_handle_for_logic_if() {
        let edges = vec![
            WorkflowEdge {
                id: "e1".into(),
                source: "if1".into(),
                target: "yes".into(),
                source_handle: Some("true".into()),
            },
            WorkflowEdge {
                id: "e2".into(),
                source: "if1".into(),
                target: "no".into(),
                source_handle: Some("false".into()),
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
}
