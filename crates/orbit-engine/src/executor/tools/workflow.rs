use serde_json::{json, Map, Value};
use ulid::Ulid;

use crate::commands::project_workflows::{
    create_project_workflow_with_db, delete_project_workflow_with_db,
    generate_reference_key_for_new_node, normalize_graph_for_storage,
    set_project_workflow_enabled_with_db, update_project_workflow_with_db,
    workflow_has_trigger_node, workflow_node_default_data, workflow_runtime_warnings,
};
use crate::db::DbPool;
use crate::executor::llm_provider::ToolDefinition;
use crate::models::project_workflow::{
    CreateProjectWorkflow, NodePosition, UpdateProjectWorkflow, WorkflowEdge, WorkflowGraph,
    WorkflowNode,
};
use crate::workflows::orchestrator::{cancel_run, list_runs_for_workflow, load_run_with_steps};
use crate::workflows::WorkflowOrchestrator;

use super::{context::ToolExecutionContext, ToolHandler};

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;
const DEFAULT_RELATIVE_DISTANCE: f64 = 240.0;

pub struct WorkflowTool;

#[async_trait::async_trait]
impl ToolHandler for WorkflowTool {
    fn name(&self) -> &'static str {
        "workflow"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Manage project workflows for the current session's project or a specified project. Supports workflow CRUD, graph edits, runs, enable/disable, and run history. Prefer this when the user wants agents to create, inspect, modify, or run project workflows.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "list", "get", "create", "update", "delete", "set_enabled",
                            "replace_graph", "add_node", "move_node", "update_node_data",
                            "replace_node_data", "delete_node", "connect_nodes", "delete_edge",
                            "run", "list_runs", "get_run", "cancel_run"
                        ]
                    },
                    "project_id": { "type": "string" },
                    "workflow_id": { "type": "string" },
                    "run_id": { "type": "string" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "enabled": { "type": "boolean" },
                    "graph": { "type": "object" },
                    "node_id": { "type": "string" },
                    "node_type": { "type": "string" },
                    "data": { "type": "object" },
                    "data_patch": { "type": "object" },
                    "placement": { "type": "object" },
                    "edge_id": { "type": "string" },
                    "source_node_id": { "type": "string" },
                    "target_node_id": { "type": "string" },
                    "source_handle": { "type": "string" },
                    "target_handle": { "type": "string" },
                    "trigger_data": { "type": "object" },
                    "limit": { "type": "integer" }
                },
                "required": ["action"]
            }),
        }
    }

    async fn execute(
        &self,
        ctx: &ToolExecutionContext,
        input: &Value,
        app: &tauri::AppHandle,
        _run_id: &str,
    ) -> Result<(String, bool), String> {
        let db = ctx.db.as_ref().ok_or("workflow: no database available")?;
        let repos = ctx
            .repos
            .as_ref()
            .ok_or("workflow: no repositories available")?;
        let action = input["action"]
            .as_str()
            .ok_or("workflow: missing 'action' field")?;

        let response = match action {
            "list" => {
                let project_id = resolve_project_id(ctx, input, None).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let limit = parse_limit(input.get("limit"));
                let workflows = repos.project_workflows().list(&project_id, limit).await?;
                let items: Vec<Value> = workflows
                    .into_iter()
                    .map(|workflow| {
                        let warnings = workflow_runtime_warnings(&workflow.graph);
                        json!({
                            "workflow": workflow,
                            "warnings": warnings,
                        })
                    })
                    .collect();
                envelope("ok", Vec::new(), json!({ "workflows": items }))?
            }
            "get" => {
                let workflow_id = required_str(input, "workflow_id", "get")?;
                let project_id = resolve_workflow_project_id(ctx, input, workflow_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let workflow = repos.project_workflows().get(workflow_id).await?;
                let warnings = workflow_runtime_warnings(&workflow.graph);
                envelope("ok", warnings, json!({ "workflow": workflow }))?
            }
            "create" => {
                let project_id = resolve_project_id(ctx, input, None).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let name = required_str(input, "name", "create")?.to_string();
                let description = optional_trimmed(input.get("description"));
                let graph = input
                    .get("graph")
                    .filter(|value| !value.is_null())
                    .map(parse_graph)
                    .transpose()?;
                let workflow = create_project_workflow_with_db(
                    db,
                    ctx.cloud_client.clone(),
                    CreateProjectWorkflow {
                        project_id,
                        name,
                        description,
                        trigger_kind: None,
                        trigger_config: None,
                        graph,
                    },
                )
                .await?;
                let warnings = workflow_runtime_warnings(&workflow.graph);
                envelope("created", warnings, json!({ "workflow": workflow }))?
            }
            "update" => {
                let workflow_id = required_str(input, "workflow_id", "update")?;
                let project_id = resolve_workflow_project_id(ctx, input, workflow_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let workflow = update_project_workflow_with_db(
                    db,
                    ctx.cloud_client.clone(),
                    workflow_id,
                    UpdateProjectWorkflow {
                        name: input
                            .get("name")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        description: match input.get("description") {
                            Some(value) if value.is_null() => Some(None),
                            Some(value) => {
                                Some(Some(value.as_str().map(str::to_string).ok_or(
                                    "workflow: update description must be a string or null",
                                )?))
                            }
                            None => None,
                        },
                        trigger_kind: None,
                        trigger_config: None,
                        graph: None,
                    },
                )
                .await?;
                let warnings = workflow_runtime_warnings(&workflow.graph);
                envelope("updated", warnings, json!({ "workflow": workflow }))?
            }
            "delete" => {
                let workflow_id = required_str(input, "workflow_id", "delete")?;
                let project_id = resolve_workflow_project_id(ctx, input, workflow_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                delete_project_workflow_with_db(db, ctx.cloud_client.clone(), workflow_id).await?;
                envelope(
                    "deleted",
                    Vec::new(),
                    json!({
                        "workflowId": workflow_id,
                        "projectId": project_id,
                    }),
                )?
            }
            "set_enabled" => {
                let workflow_id = required_str(input, "workflow_id", "set_enabled")?;
                let enabled = input["enabled"]
                    .as_bool()
                    .ok_or("workflow: set_enabled requires 'enabled'")?;
                let project_id = resolve_workflow_project_id(ctx, input, workflow_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let workflow = set_project_workflow_enabled_with_db(
                    db,
                    ctx.cloud_client.clone(),
                    workflow_id,
                    enabled,
                )
                .await?;
                let warnings = workflow_runtime_warnings(&workflow.graph);
                envelope(
                    if enabled { "enabled" } else { "disabled" },
                    warnings,
                    json!({ "workflow": workflow }),
                )?
            }
            "replace_graph" => {
                let workflow_id = required_str(input, "workflow_id", "replace_graph")?;
                let project_id = resolve_workflow_project_id(ctx, input, workflow_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let workflow = update_project_workflow_with_db(
                    db,
                    ctx.cloud_client.clone(),
                    workflow_id,
                    UpdateProjectWorkflow {
                        name: None,
                        description: None,
                        trigger_kind: None,
                        trigger_config: None,
                        graph: Some(parse_graph(
                            input
                                .get("graph")
                                .ok_or("workflow: replace_graph requires 'graph'")?,
                        )?),
                    },
                )
                .await?;
                let warnings = workflow_runtime_warnings(&workflow.graph);
                envelope("graph_replaced", warnings, json!({ "workflow": workflow }))?
            }
            "add_node" => {
                let workflow_id = required_str(input, "workflow_id", "add_node")?;
                let project_id = resolve_workflow_project_id(ctx, input, workflow_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let mut workflow = repos.project_workflows().get(workflow_id).await?;
                let node_type = required_str(input, "node_type", "add_node")?;
                let mut data = workflow_node_default_data(node_type)
                    .ok_or_else(|| format!("workflow: unknown node type '{}'", node_type))?;
                if let Some(patch) = input.get("data") {
                    ensure_object(patch, "workflow: add_node data must be an object")?;
                    merge_patch(&mut data, patch);
                }
                let data_obj = data
                    .as_object_mut()
                    .ok_or("workflow: node data must be an object")?;
                if data_obj
                    .get("referenceKey")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .is_empty()
                {
                    data_obj.insert(
                        "referenceKey".to_string(),
                        Value::String(generate_reference_key_for_new_node(
                            node_type,
                            &workflow.graph.nodes,
                        )),
                    );
                }

                let position = resolve_position(
                    &workflow.graph.nodes,
                    input
                        .get("placement")
                        .ok_or("workflow: add_node requires 'placement'")?,
                )?;
                let node = WorkflowNode {
                    id: format!("n_{}", Ulid::new()),
                    node_type: node_type.to_string(),
                    position,
                    data,
                };
                let created_node_id = node.id.clone();
                workflow.graph.nodes.push(node);
                let workflow = persist_graph(db, ctx, workflow_id, workflow.graph).await?;
                let warnings = workflow_runtime_warnings(&workflow.graph);
                envelope(
                    "node_added",
                    warnings,
                    json!({
                        "workflow": workflow,
                        "createdNodeId": created_node_id,
                    }),
                )?
            }
            "move_node" => {
                let workflow_id = required_str(input, "workflow_id", "move_node")?;
                let project_id = resolve_workflow_project_id(ctx, input, workflow_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let node_id = required_str(input, "node_id", "move_node")?;
                let mut workflow = repos.project_workflows().get(workflow_id).await?;
                let position = resolve_position(
                    &workflow.graph.nodes,
                    input
                        .get("placement")
                        .ok_or("workflow: move_node requires 'placement'")?,
                )?;
                let node = workflow
                    .graph
                    .nodes
                    .iter_mut()
                    .find(|node| node.id == node_id)
                    .ok_or_else(|| format!("workflow: node '{}' not found", node_id))?;
                node.position = position;
                let workflow = persist_graph(db, ctx, workflow_id, workflow.graph).await?;
                let warnings = workflow_runtime_warnings(&workflow.graph);
                envelope("node_moved", warnings, json!({ "workflow": workflow }))?
            }
            "update_node_data" => {
                let workflow_id = required_str(input, "workflow_id", "update_node_data")?;
                let project_id = resolve_workflow_project_id(ctx, input, workflow_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let node_id = required_str(input, "node_id", "update_node_data")?;
                let patch = input
                    .get("data_patch")
                    .ok_or("workflow: update_node_data requires 'data_patch'")?;
                ensure_object(patch, "workflow: data_patch must be an object")?;
                let mut workflow = repos.project_workflows().get(workflow_id).await?;
                let node = workflow
                    .graph
                    .nodes
                    .iter_mut()
                    .find(|node| node.id == node_id)
                    .ok_or_else(|| format!("workflow: node '{}' not found", node_id))?;
                let mut data = match node.data.clone() {
                    Value::Object(_) => node.data.clone(),
                    _ => json!({}),
                };
                merge_patch(&mut data, patch);
                ensure_object(&data, "workflow: patched node data must be an object")?;
                node.data = data;
                let workflow = persist_graph(db, ctx, workflow_id, workflow.graph).await?;
                let warnings = workflow_runtime_warnings(&workflow.graph);
                envelope("node_updated", warnings, json!({ "workflow": workflow }))?
            }
            "replace_node_data" => {
                let workflow_id = required_str(input, "workflow_id", "replace_node_data")?;
                let project_id = resolve_workflow_project_id(ctx, input, workflow_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let node_id = required_str(input, "node_id", "replace_node_data")?;
                let data = input
                    .get("data")
                    .ok_or("workflow: replace_node_data requires 'data'")?
                    .clone();
                ensure_object(&data, "workflow: replace_node_data data must be an object")?;
                let mut workflow = repos.project_workflows().get(workflow_id).await?;
                let node = workflow
                    .graph
                    .nodes
                    .iter_mut()
                    .find(|node| node.id == node_id)
                    .ok_or_else(|| format!("workflow: node '{}' not found", node_id))?;
                node.data = data;
                let workflow = persist_graph(db, ctx, workflow_id, workflow.graph).await?;
                let warnings = workflow_runtime_warnings(&workflow.graph);
                envelope("node_replaced", warnings, json!({ "workflow": workflow }))?
            }
            "delete_node" => {
                let workflow_id = required_str(input, "workflow_id", "delete_node")?;
                let project_id = resolve_workflow_project_id(ctx, input, workflow_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let node_id = required_str(input, "node_id", "delete_node")?;
                let mut workflow = repos.project_workflows().get(workflow_id).await?;
                let before = workflow.graph.nodes.len();
                workflow.graph.nodes.retain(|node| node.id != node_id);
                if workflow.graph.nodes.len() == before {
                    return Err(format!("workflow: node '{}' not found", node_id));
                }
                workflow
                    .graph
                    .edges
                    .retain(|edge| edge.source != node_id && edge.target != node_id);
                let workflow = persist_graph(db, ctx, workflow_id, workflow.graph).await?;
                let warnings = workflow_runtime_warnings(&workflow.graph);
                envelope(
                    "node_deleted",
                    warnings,
                    json!({
                        "workflow": workflow,
                        "deletedNodeId": node_id,
                    }),
                )?
            }
            "connect_nodes" => {
                let workflow_id = required_str(input, "workflow_id", "connect_nodes")?;
                let project_id = resolve_workflow_project_id(ctx, input, workflow_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let source_node_id = required_str(input, "source_node_id", "connect_nodes")?;
                let target_node_id = required_str(input, "target_node_id", "connect_nodes")?;
                let mut workflow = repos.project_workflows().get(workflow_id).await?;
                let source_node = workflow
                    .graph
                    .nodes
                    .iter()
                    .find(|node| node.id == source_node_id)
                    .ok_or_else(|| {
                        format!("workflow: source node '{}' not found", source_node_id)
                    })?;
                if workflow
                    .graph
                    .nodes
                    .iter()
                    .all(|node| node.id != target_node_id)
                {
                    return Err(format!(
                        "workflow: target node '{}' not found",
                        target_node_id
                    ));
                }
                let edge = WorkflowEdge {
                    id: format!("e_{}", Ulid::new()),
                    source: source_node_id.to_string(),
                    target: target_node_id.to_string(),
                    source_handle: input
                        .get("source_handle")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| {
                            if source_node.node_type == "logic.if" {
                                Some("true".to_string())
                            } else {
                                None
                            }
                        }),
                    target_handle: input
                        .get("target_handle")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                };
                let created_edge_id = edge.id.clone();
                workflow.graph.edges.push(edge);
                let workflow = persist_graph(db, ctx, workflow_id, workflow.graph).await?;
                let warnings = workflow_runtime_warnings(&workflow.graph);
                envelope(
                    "edge_connected",
                    warnings,
                    json!({
                        "workflow": workflow,
                        "createdEdgeId": created_edge_id,
                    }),
                )?
            }
            "delete_edge" => {
                let workflow_id = required_str(input, "workflow_id", "delete_edge")?;
                let project_id = resolve_workflow_project_id(ctx, input, workflow_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let edge_id = required_str(input, "edge_id", "delete_edge")?;
                let mut workflow = repos.project_workflows().get(workflow_id).await?;
                let before = workflow.graph.edges.len();
                workflow.graph.edges.retain(|edge| edge.id != edge_id);
                if workflow.graph.edges.len() == before {
                    return Err(format!("workflow: edge '{}' not found", edge_id));
                }
                let workflow = persist_graph(db, ctx, workflow_id, workflow.graph).await?;
                let warnings = workflow_runtime_warnings(&workflow.graph);
                envelope(
                    "edge_deleted",
                    warnings,
                    json!({
                        "workflow": workflow,
                        "deletedEdgeId": edge_id,
                    }),
                )?
            }
            "run" => {
                let workflow_id = required_str(input, "workflow_id", "run")?;
                let project_id = resolve_workflow_project_id(ctx, input, workflow_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let workflow = repos.project_workflows().get(workflow_id).await?;
                if !workflow_has_trigger_node(&workflow.graph) {
                    return Err(structured_error(
                        "workflow_missing_trigger",
                        "workflow cannot run without a trigger node".to_string(),
                    ));
                }
                let warnings = workflow_runtime_warnings(&workflow.graph);
                let trigger_data = input.get("trigger_data").cloned().unwrap_or(Value::Null);
                let run = WorkflowOrchestrator::new(
                    db.clone(),
                    crate::runtime_host::tauri_host(app.clone()),
                )
                .start_run(workflow_id.to_string(), "manual", trigger_data)
                .await?;
                envelope(
                    "run_started",
                    warnings,
                    json!({ "run": run, "workflow": workflow }),
                )?
            }
            "list_runs" => {
                let workflow_id = required_str(input, "workflow_id", "list_runs")?;
                let project_id = resolve_workflow_project_id(ctx, input, workflow_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let workflow = repos.project_workflows().get(workflow_id).await?;
                let warnings = workflow_runtime_warnings(&workflow.graph);
                let limit = parse_limit(input.get("limit"));
                let runs = list_runs_for_workflow(db, workflow_id, limit)?;
                envelope("ok", warnings, json!({ "runs": runs }))?
            }
            "get_run" => {
                let run_id = required_str(input, "run_id", "get_run")?;
                let (_workflow_id, project_id) = resolve_run_scope(ctx, run_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let run = load_run_with_steps(db, run_id)?;
                let warnings = run_runtime_warnings(&run);
                envelope("ok", warnings, json!({ "run": run }))?
            }
            "cancel_run" => {
                let run_id = required_str(input, "run_id", "cancel_run")?;
                let (_workflow_id, project_id) = resolve_run_scope(ctx, run_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                cancel_run(db, run_id)?;
                let run = load_run_with_steps(db, run_id)?;
                let warnings = run_runtime_warnings(&run);
                envelope("cancelled", warnings, json!({ "run": run }))?
            }
            other => return Err(format!("workflow: unknown action '{}'", other)),
        };

        Ok((response, false))
    }
}

fn parse_limit(value: Option<&Value>) -> i64 {
    value
        .and_then(Value::as_i64)
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT)
}

fn required_str<'a>(input: &'a Value, field: &str, action: &str) -> Result<&'a str, String> {
    input[field]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("workflow: {} requires '{}'", action, field))
}

fn optional_trimmed(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn ensure_object(value: &Value, message: &str) -> Result<(), String> {
    if value.is_object() {
        Ok(())
    } else {
        Err(message.to_string())
    }
}

fn structured_error(code: &str, message: String) -> String {
    json!({
        "code": code,
        "message": message,
    })
    .to_string()
}

async fn resolve_project_id(
    ctx: &ToolExecutionContext,
    input: &Value,
    explicit: Option<&str>,
) -> Result<String, String> {
    if let Some(project_id) = explicit {
        return Ok(project_id.to_string());
    }
    if let Some(project_id) = input["project_id"].as_str() {
        if !project_id.is_empty() {
            return Ok(project_id.to_string());
        }
    }
    let Some(session_id) = ctx.current_session_id.as_deref() else {
        return Err("workflow: no project_id provided and no current session to infer from".into());
    };
    let repos = ctx
        .repos
        .as_ref()
        .ok_or("workflow: no repositories available")?;
    let project_id = repos.chat().session_meta(session_id).await?.project_id;
    project_id.ok_or_else(|| {
        "workflow: no project_id provided and current session is not scoped to a project"
            .to_string()
    })
}

async fn resolve_workflow_project_id(
    ctx: &ToolExecutionContext,
    input: &Value,
    workflow_id: &str,
) -> Result<String, String> {
    if let Some(project_id) = input["project_id"].as_str() {
        if !project_id.is_empty() {
            return Ok(project_id.to_string());
        }
    }
    let repos = ctx
        .repos
        .as_ref()
        .ok_or("workflow: no repositories available")?;
    repos
        .project_workflows()
        .lookup_project_id(workflow_id)
        .await
}

async fn resolve_run_scope(
    ctx: &ToolExecutionContext,
    run_id: &str,
) -> Result<(String, String), String> {
    let repos = ctx
        .repos
        .as_ref()
        .ok_or("workflow: no repositories available")?;
    repos.project_workflows().lookup_run_scope(run_id).await
}

async fn enforce_project_scope(ctx: &ToolExecutionContext, project_id: &str) -> Result<(), String> {
    let repos = ctx
        .repos
        .as_ref()
        .ok_or("workflow: no repositories available")?;
    let in_project = repos
        .projects()
        .agent_in_project(project_id, &ctx.agent_id)
        .await?;
    if !in_project {
        return Err(structured_error(
            "agent_not_in_project",
            format!("agent is not a member of project '{}'", project_id),
        ));
    }
    Ok(())
}

fn parse_graph(value: &Value) -> Result<WorkflowGraph, String> {
    serde_json::from_value(value.clone())
        .map_err(|e| format!("workflow: invalid graph payload: {}", e))
}

async fn persist_graph(
    db: &DbPool,
    ctx: &ToolExecutionContext,
    workflow_id: &str,
    graph: WorkflowGraph,
) -> Result<crate::models::project_workflow::ProjectWorkflow, String> {
    update_project_workflow_with_db(
        db,
        ctx.cloud_client.clone(),
        workflow_id,
        UpdateProjectWorkflow {
            name: None,
            description: None,
            trigger_kind: None,
            trigger_config: None,
            graph: Some(normalize_graph_for_storage(&graph)),
        },
    )
    .await
}

fn envelope(status: &str, warnings: Vec<String>, payload: Value) -> Result<String, String> {
    let mut out = Map::new();
    out.insert("status".to_string(), Value::String(status.to_string()));
    out.insert("warnings".to_string(), json!(warnings));
    match payload {
        Value::Object(map) => {
            for (key, value) in map {
                out.insert(key, value);
            }
        }
        other => {
            out.insert("result".to_string(), other);
        }
    }
    serde_json::to_string_pretty(&Value::Object(out))
        .map_err(|e| format!("workflow: serialize: {}", e))
}

fn merge_patch(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(target_obj), Value::Object(patch_obj)) => {
            for (key, patch_value) in patch_obj {
                if patch_value.is_null() {
                    target_obj.remove(key);
                    continue;
                }
                match target_obj.get_mut(key) {
                    Some(target_value) if target_value.is_object() && patch_value.is_object() => {
                        merge_patch(target_value, patch_value);
                    }
                    _ => {
                        target_obj.insert(key.clone(), patch_value.clone());
                    }
                }
            }
        }
        (target_value, patch_value) => {
            *target_value = patch_value.clone();
        }
    }
}

fn resolve_position(nodes: &[WorkflowNode], placement: &Value) -> Result<NodePosition, String> {
    let mode = placement["mode"]
        .as_str()
        .ok_or("workflow: placement requires 'mode'")?;
    match mode {
        "absolute" => Ok(NodePosition {
            x: placement["x"]
                .as_f64()
                .ok_or("workflow: absolute placement requires numeric 'x'")?,
            y: placement["y"]
                .as_f64()
                .ok_or("workflow: absolute placement requires numeric 'y'")?,
        }),
        "relative" => {
            let relative_to_node_id = placement["relative_to_node_id"]
                .as_str()
                .ok_or("workflow: relative placement requires 'relative_to_node_id'")?;
            let direction = placement["direction"]
                .as_str()
                .ok_or("workflow: relative placement requires 'direction'")?;
            let distance = placement["distance"]
                .as_f64()
                .unwrap_or(DEFAULT_RELATIVE_DISTANCE);
            let offset_x = placement["offset_x"].as_f64().unwrap_or(0.0);
            let offset_y = placement["offset_y"].as_f64().unwrap_or(0.0);
            let anchor = nodes
                .iter()
                .find(|node| node.id == relative_to_node_id)
                .ok_or_else(|| {
                    format!(
                        "workflow: relative placement node '{}' not found",
                        relative_to_node_id
                    )
                })?;
            let (dx, dy) = match direction {
                "right" => (distance, 0.0),
                "left" => (-distance, 0.0),
                "above" => (0.0, -distance),
                "below" => (0.0, distance),
                _ => return Err(
                    "workflow: relative placement direction must be one of right/left/above/below"
                        .to_string(),
                ),
            };
            Ok(NodePosition {
                x: anchor.position.x + dx + offset_x,
                y: anchor.position.y + dy + offset_y,
            })
        }
        _ => Err("workflow: placement mode must be 'absolute' or 'relative'".to_string()),
    }
}

fn run_runtime_warnings(
    run: &(
        crate::models::workflow_run::WorkflowRun,
        Vec<crate::models::workflow_run::WorkflowRunStep>,
    ),
) -> Vec<String> {
    serde_json::from_value::<WorkflowGraph>(run.0.graph_snapshot.clone())
        .map(|graph| workflow_runtime_warnings(&graph))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{merge_patch, resolve_position, WorkflowTool};
    use crate::executor::tools::ToolHandler;
    use crate::models::project_workflow::{NodePosition, WorkflowNode};
    use serde_json::json;

    #[test]
    fn merge_patch_deletes_keys_and_replaces_arrays() {
        let mut target = json!({
            "a": 1,
            "nested": { "keep": true, "drop": "x" },
            "items": [1, 2],
        });
        let patch = json!({
            "nested": { "drop": null, "new": "y" },
            "items": [3],
        });

        merge_patch(&mut target, &patch);

        assert_eq!(
            target,
            json!({
                "a": 1,
                "nested": { "keep": true, "new": "y" },
                "items": [3],
            })
        );
    }

    #[test]
    fn resolve_relative_position_uses_direction_distance_and_offsets() {
        let nodes = vec![WorkflowNode {
            id: "node-1".into(),
            node_type: "trigger.manual".into(),
            position: NodePosition { x: 100.0, y: 80.0 },
            data: json!({}),
        }];

        let position = resolve_position(
            &nodes,
            &json!({
                "mode": "relative",
                "relative_to_node_id": "node-1",
                "direction": "right",
                "distance": 200,
                "offset_y": 10,
            }),
        )
        .unwrap();

        assert_eq!(position.x, 300.0);
        assert_eq!(position.y, 90.0);
    }

    #[test]
    fn workflow_tool_definition_uses_expected_name() {
        let tool = WorkflowTool;
        assert_eq!(tool.definition().name, "workflow");
    }
}
