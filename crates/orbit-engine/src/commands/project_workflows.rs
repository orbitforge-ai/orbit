use crate::app_context::AppContext;
use crate::db::cloud::SupabaseClient;
use crate::models::project_workflow::{
    CreateProjectWorkflow, ProjectWorkflow, RuleNode, UpdateProjectWorkflow, WorkflowEdge,
    WorkflowGraph, WorkflowNode, KNOWN_NODE_TYPES, RULE_OPERATORS,
};
use crate::workflows::nodes::node_type_has_executor;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// ── Cloud helper ──────────────────────────────────────────────────────────────

#[allow(unused_macros)]
macro_rules! cloud_upsert_workflow {
    ($cloud:expr, $wf:expr) => {
        if let Some(client) = $cloud.get() {
            let w = $wf.clone();
            tokio::spawn(async move {
                if let Err(e) = client.upsert_project_workflow(&w).await {
                    tracing::warn!("cloud upsert project_workflow: {}", e);
                }
            });
        }
    };
}

#[allow(unused_macros)]
macro_rules! cloud_delete {
    ($cloud:expr, $table:expr, $id:expr) => {
        if let Some(client) = $cloud.get() {
            let id = $id.to_string();
            tokio::spawn(async move {
                if let Err(e) = client.delete_by_id($table, &id).await {
                    tracing::warn!("cloud delete {}: {}", $table, e);
                }
            });
        }
    };
}

// ── Validation ────────────────────────────────────────────────────────────────
//
// Save-time validation rules (per the plan, §3 / §8):
// - All node `type` values must be in KNOWN_NODE_TYPES.
// - Edges must reference existing node ids.
// - `logic.if` nodes may have zero or one outgoing edge per branch handle
//   (`sourceHandle` "true" / "false"), and any rule tree they carry must
//   use only known operators and combinators.
// - No fan-in: every non-trigger node may have at most one incoming edge.

pub fn validate_graph(graph: &WorkflowGraph) -> Result<(), String> {
    let node_ids: HashSet<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    if node_ids.len() != graph.nodes.len() {
        return Err("workflow: duplicate node ids".into());
    }

    let mut reference_keys = HashSet::new();

    for node in &graph.nodes {
        if !KNOWN_NODE_TYPES.contains(&node.node_type.as_str()) {
            return Err(format!(
                "workflow: unknown node type '{}' on node '{}'",
                node.node_type, node.id
            ));
        }
        if let Some(reference_key) = workflow_node_reference_key(&node.data) {
            if !is_valid_reference_key(reference_key) {
                return Err(format!(
                    "workflow: node '{}' has invalid referenceKey '{}'; use kebab-case letters, numbers, and hyphens",
                    node.id, reference_key
                ));
            }
            if !reference_keys.insert(reference_key.to_string()) {
                return Err(format!(
                    "workflow: duplicate referenceKey '{}' found; each node reference name must be unique",
                    reference_key
                ));
            }
        }
        if node.node_type == "logic.if" {
            if let Some(rule_value) = node.data.get("rule") {
                let parsed: Result<RuleNode, _> = serde_json::from_value(rule_value.clone());
                match parsed {
                    Ok(rule) => validate_rule(&rule)?,
                    Err(e) => {
                        return Err(format!(
                            "workflow: logic.if node '{}' has malformed rule: {}",
                            node.id, e
                        ))
                    }
                }
            }
        }
    }

    // Per-target incoming edge count + per-source outgoing edge count for logic.if
    let mut incoming: HashMap<&str, usize> = HashMap::new();
    let mut logic_if_handles: HashMap<&str, HashSet<String>> = HashMap::new();
    let logic_if_ids: HashSet<&str> = graph
        .nodes
        .iter()
        .filter(|n| n.node_type == "logic.if")
        .map(|n| n.id.as_str())
        .collect();

    for edge in &graph.edges {
        if !node_ids.contains(edge.source.as_str()) {
            return Err(format!(
                "workflow: edge '{}' references unknown source node '{}'",
                edge.id, edge.source
            ));
        }
        if !node_ids.contains(edge.target.as_str()) {
            return Err(format!(
                "workflow: edge '{}' references unknown target node '{}'",
                edge.id, edge.target
            ));
        }
        *incoming.entry(edge.target.as_str()).or_insert(0) += 1;
        if logic_if_ids.contains(edge.source.as_str()) {
            let handle = edge.source_handle.clone().unwrap_or_default();
            if handle != "true" && handle != "false" {
                return Err(format!(
                    "workflow: logic.if node '{}' has outgoing edge '{}' with invalid handle '{}', expected 'true' or 'false'",
                    edge.source, edge.id, handle
                ));
            }
            let inserted = logic_if_handles
                .entry(edge.source.as_str())
                .or_default()
                .insert(handle.clone());
            if !inserted {
                return Err(format!(
                    "workflow: logic.if node '{}' has multiple outgoing '{}' edges; each branch may only connect once",
                    edge.source, handle
                ));
            }
        }
    }

    // Fan-in check (no joins in v1).
    for (target, count) in &incoming {
        if *count > 1 {
            return Err(format!(
                "workflow: node '{}' has {} incoming edges; fan-in / join nodes are not supported",
                target, count
            ));
        }
    }

    Ok(())
}

fn workflow_node_reference_key(data: &Value) -> Option<&str> {
    data.get("referenceKey")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn is_valid_reference_key(value: &str) -> bool {
    if value == "trigger" || value == "__aliases" || value.starts_with('-') || value.ends_with('-')
    {
        return false;
    }
    let mut saw_alnum = false;
    for ch in value.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            saw_alnum = true;
            continue;
        }
        if ch != '-' {
            return false;
        }
    }
    saw_alnum
}

fn validate_rule(rule: &RuleNode) -> Result<(), String> {
    match rule {
        RuleNode::Group(group) => {
            if group.combinator != "and" && group.combinator != "or" {
                return Err(format!(
                    "workflow: unknown rule combinator '{}'",
                    group.combinator
                ));
            }
            for child in &group.rules {
                validate_rule(child)?;
            }
            Ok(())
        }
        RuleNode::Leaf(leaf) => {
            if !RULE_OPERATORS.contains(&leaf.operator.as_str()) {
                return Err(format!(
                    "workflow: unknown rule operator '{}'",
                    leaf.operator
                ));
            }
            Ok(())
        }
    }
}

pub fn workflow_has_trigger_node(graph: &WorkflowGraph) -> bool {
    graph
        .nodes
        .iter()
        .any(|node| node.node_type.starts_with("trigger."))
}

pub fn workflow_runtime_warnings(graph: &WorkflowGraph) -> Vec<String> {
    let mut by_type: HashMap<&str, Vec<&str>> = HashMap::new();
    for node in &graph.nodes {
        if !KNOWN_NODE_TYPES.contains(&node.node_type.as_str())
            || node_type_has_executor(&node.node_type)
        {
            continue;
        }
        by_type
            .entry(node.node_type.as_str())
            .or_default()
            .push(node.id.as_str());
    }

    let mut warnings = Vec::new();
    let mut entries: Vec<_> = by_type.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    for (node_type, node_ids) in entries {
        warnings.push(format!(
            "workflow contains runtime-inert node type '{}' on node(s): {}",
            node_type,
            node_ids.join(", ")
        ));
    }
    warnings
}

pub fn workflow_node_default_data(node_type: &str) -> Option<Value> {
    Some(match node_type {
        "trigger.manual" => json!({}),
        "trigger.schedule" => json!({
            "intervalUnit": "hours",
            "intervalValue": 1,
            "timezone": "UTC",
            "missedRunPolicy": "skip",
        }),
        "agent.run" => json!({
            "agentId": "",
            "promptTemplate": "",
            "contextTemplate": "",
            "outputMode": "text",
        }),
        "logic.if" => json!({
            "rule": { "combinator": "and", "rules": [] },
            "trueLabel": "true",
            "falseLabel": "false",
        }),
        "code.bash.run" => json!({
            "script": "",
            "workingDirectory": ".",
            "timeoutSeconds": 120,
        }),
        "code.script.run" => json!({
            "language": "typescript",
            "source": "",
            "workingDirectory": ".",
            "timeoutSeconds": 120,
        }),
        "board.work_item.create" => json!({
            "action": "create",
            "itemIdTemplate": "",
            "titleTemplate": "",
            "descriptionTemplate": "",
            "columnId": "",
            "kind": "",
            "status": "",
            "priority": Value::Null,
            "labelsText": "",
            "assigneeAgentId": "",
            "parentWorkItemId": "",
            "reasonTemplate": "",
            "bodyTemplate": "",
            "commentAuthorAgentId": "",
            "listColumn": "all",
            "listStatus": "all",
            "listKind": "all",
            "listAssignee": "",
            "limit": 25,
        }),
        "board.proposal.enqueue" => json!({
            "candidatesPath": "",
            "reviewColumnId": "",
            "kind": "task",
            "priority": 1,
            "labelsText": "proposal-review",
        }),
        "integration.feed.fetch" => json!({
            "feedUrlsText": "",
            "limit": 50,
        }),
        "integration.com_orbit_discord.send_message" => json!({
            "channelId": "",
            "threadId": "",
            "text": "",
        }),
        "integration.gmail.read" | "integration.gmail.send" | "integration.slack.send" => {
            json!({})
        }
        "integration.http.request" => json!({
            "method": "GET",
            "url": "",
        }),
        _ => return None,
    })
}

fn node_type_label(node_type: &str) -> String {
    match node_type {
        "trigger.manual" => "Run now".to_string(),
        "trigger.schedule" => "Schedule".to_string(),
        "agent.run" => "Run agent".to_string(),
        "logic.if" => "If / branch".to_string(),
        "code.bash.run" => "Code · Bash".to_string(),
        "code.script.run" => "Code · JS/TS".to_string(),
        "board.work_item.create" => "Board · Work item".to_string(),
        "board.proposal.enqueue" => "Board · Proposal queue".to_string(),
        "integration.feed.fetch" => "Feed fetch".to_string(),
        "integration.com_orbit_discord.send_message" => "Discord · Send message".to_string(),
        "integration.gmail.read" => "Gmail · Read".to_string(),
        "integration.gmail.send" => "Gmail · Send".to_string(),
        "integration.slack.send" => "Slack · Send".to_string(),
        "integration.http.request" => "HTTP request".to_string(),
        other => other.replace('.', " "),
    }
}

fn normalize_data_object(value: &Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map.clone(),
        _ => Map::new(),
    }
}

fn slugify_reference_key(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.trim().to_lowercase().chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn normalize_candidate_reference_key(value: Option<&str>) -> Option<String> {
    let normalized = slugify_reference_key(value.unwrap_or_default());
    if normalized.is_empty() || normalized == "trigger" || normalized == "__aliases" {
        return None;
    }
    Some(normalized)
}

fn auto_reference_base(node_type: &str, preferred: Option<&str>) -> String {
    normalize_candidate_reference_key(preferred)
        .unwrap_or_else(|| slugify_reference_key(&node_type_label(node_type)))
        .chars()
        .collect::<String>()
}

fn is_generated_reference_key(node_type: &str, value: &str) -> bool {
    let base = auto_reference_base(node_type, None);
    value == base
        || value
            .strip_prefix(&(base + "-"))
            .map(|suffix| suffix.chars().all(|ch| ch.is_ascii_digit()))
            .unwrap_or(false)
}

fn node_has_linked_outputs(node_id: &str, node_type: &str, edges: &[WorkflowEdge]) -> bool {
    node_type.starts_with("trigger.") || edges.iter().any(|edge| edge.source == node_id)
}

pub fn generate_reference_key_for_new_node(node_type: &str, nodes: &[WorkflowNode]) -> String {
    let base = auto_reference_base(node_type, None);
    let mut max_suffix = 0usize;
    let mut same_type_count = 0usize;
    for node in nodes {
        if node.node_type != node_type {
            continue;
        }
        same_type_count += 1;
        let data = normalize_data_object(&node.data);
        let Some(existing) = data.get("referenceKey").and_then(Value::as_str) else {
            continue;
        };
        let Some(existing) = normalize_candidate_reference_key(Some(existing)) else {
            continue;
        };
        let Some(suffix) = existing.strip_prefix(&(base.clone() + "-")) else {
            continue;
        };
        let Ok(parsed) = suffix.parse::<usize>() else {
            continue;
        };
        max_suffix = max_suffix.max(parsed);
    }
    format!(
        "{}-{}",
        base,
        std::cmp::max(max_suffix + 1, same_type_count + 1)
    )
}

pub fn normalize_graph_for_storage(graph: &WorkflowGraph) -> WorkflowGraph {
    let mut normalized = graph.clone();
    let mut used = HashSet::from(["trigger".to_string(), "__aliases".to_string()]);

    for node in &normalized.nodes {
        if !node_has_linked_outputs(&node.id, &node.node_type, &normalized.edges) {
            continue;
        }
        let data = normalize_data_object(&node.data);
        let Some(existing) = data.get("referenceKey").and_then(Value::as_str) else {
            continue;
        };
        let Some(existing) = normalize_candidate_reference_key(Some(existing)) else {
            continue;
        };
        if !is_generated_reference_key(&node.node_type, &existing) {
            used.insert(existing);
        }
    }

    for node in &mut normalized.nodes {
        let mut data = normalize_data_object(&node.data);
        let existing = data
            .get("referenceKey")
            .and_then(Value::as_str)
            .and_then(|value| normalize_candidate_reference_key(Some(value)));

        if !node_has_linked_outputs(&node.id, &node.node_type, &normalized.edges) {
            node.data = Value::Object(data);
            continue;
        }

        if let Some(existing) = existing {
            if !is_generated_reference_key(&node.node_type, &existing) {
                data.insert("referenceKey".to_string(), Value::String(existing));
                node.data = Value::Object(data);
                continue;
            }
        }

        let base = auto_reference_base(&node.node_type, None);
        let mut suffix = 1usize;
        let mut candidate = format!("{}-{}", base, suffix);
        while used.contains(&candidate) {
            suffix += 1;
            candidate = format!("{}-{}", base, suffix);
        }
        used.insert(candidate.clone());
        data.insert("referenceKey".to_string(), Value::String(candidate));
        node.data = Value::Object(data);
    }

    normalized
}

pub(crate) fn spawn_cloud_upsert_workflow(
    cloud: Option<Arc<SupabaseClient>>,
    workflow: &ProjectWorkflow,
) {
    if let Some(client) = cloud {
        let workflow = workflow.clone();
        tokio::spawn(async move {
            if let Err(err) = client.upsert_project_workflow(&workflow).await {
                tracing::warn!("cloud upsert project_workflow: {}", err);
            }
        });
    }
}

pub(crate) fn spawn_cloud_delete_workflow(cloud: Option<Arc<SupabaseClient>>, workflow_id: &str) {
    if let Some(client) = cloud {
        let workflow_id = workflow_id.to_string();
        tokio::spawn(async move {
            if let Err(err) = client.delete_by_id("project_workflows", &workflow_id).await {
                tracing::warn!("cloud delete project_workflows: {}", err);
            }
        });
    }
}

// ── Commands ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_project_workflows(
    project_id: String,
    app: tauri::State<'_, AppContext>,
) -> Result<Vec<ProjectWorkflow>, String> {
    app.repos.project_workflows().list(&project_id, 100).await
}

#[tauri::command]
pub async fn get_project_workflow(
    id: String,
    app: tauri::State<'_, AppContext>,
) -> Result<ProjectWorkflow, String> {
    app.repos.project_workflows().get(&id).await
}

#[tauri::command]
pub async fn create_project_workflow(
    payload: CreateProjectWorkflow,
    app: tauri::State<'_, AppContext>,
) -> Result<ProjectWorkflow, String> {
    create_project_workflow_inner(payload, &app).await
}

async fn create_project_workflow_inner(
    payload: CreateProjectWorkflow,
    app: &AppContext,
) -> Result<ProjectWorkflow, String> {
    let workflow = app.repos.project_workflows().create(payload).await?;
    spawn_cloud_upsert_workflow(app.cloud.get(), &workflow);
    Ok(workflow)
}

#[tauri::command]
pub async fn update_project_workflow(
    id: String,
    payload: UpdateProjectWorkflow,
    app: tauri::State<'_, AppContext>,
) -> Result<ProjectWorkflow, String> {
    update_project_workflow_inner(id, payload, &app).await
}

async fn update_project_workflow_inner(
    id: String,
    payload: UpdateProjectWorkflow,
    app: &AppContext,
) -> Result<ProjectWorkflow, String> {
    let workflow = app.repos.project_workflows().update(&id, payload).await?;
    spawn_cloud_upsert_workflow(app.cloud.get(), &workflow);
    Ok(workflow)
}

#[tauri::command]
pub async fn delete_project_workflow(
    id: String,
    app: tauri::State<'_, AppContext>,
) -> Result<(), String> {
    delete_project_workflow_inner(id, &app).await
}

async fn delete_project_workflow_inner(id: String, app: &AppContext) -> Result<(), String> {
    app.repos.project_workflows().delete(&id).await?;
    spawn_cloud_delete_workflow(app.cloud.get(), &id);
    Ok(())
}

#[tauri::command]
pub async fn set_project_workflow_enabled(
    id: String,
    enabled: bool,
    app: tauri::State<'_, AppContext>,
) -> Result<ProjectWorkflow, String> {
    set_project_workflow_enabled_inner(id, enabled, &app).await
}

async fn set_project_workflow_enabled_inner(
    id: String,
    enabled: bool,
    app: &AppContext,
) -> Result<ProjectWorkflow, String> {
    let workflow = app
        .repos
        .project_workflows()
        .set_enabled(&id, enabled)
        .await?;
    spawn_cloud_upsert_workflow(app.cloud.get(), &workflow);
    Ok(workflow)
}

mod http {
    use super::*;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ProjectIdArgs {
        project_id: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct IdArgs {
        id: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CreateArgs {
        payload: CreateProjectWorkflow,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UpdateArgs {
        id: String,
        payload: UpdateProjectWorkflow,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct EnabledArgs {
        id: String,
        enabled: bool,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("list_project_workflows", |ctx, args| async move {
            let a: ProjectIdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx
                .repos
                .project_workflows()
                .list(&a.project_id, 100)
                .await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("get_project_workflow", |ctx, args| async move {
            let a: IdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx.repos.project_workflows().get(&a.id).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("create_project_workflow", |ctx, args| async move {
            let a: CreateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = create_project_workflow_inner(a.payload, &ctx).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("update_project_workflow", |ctx, args| async move {
            let a: UpdateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = update_project_workflow_inner(a.id, a.payload, &ctx).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("delete_project_workflow", |ctx, args| async move {
            let a: IdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            delete_project_workflow_inner(a.id, &ctx).await?;
            Ok(serde_json::Value::Null)
        });
        reg.register("set_project_workflow_enabled", |ctx, args| async move {
            let a: EnabledArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = set_project_workflow_enabled_inner(a.id, a.enabled, &ctx).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
    }
}

pub use http::register as register_http;

#[cfg(test)]
mod tests {
    use super::{validate_graph, workflow_node_default_data, workflow_runtime_warnings};
    use crate::models::project_workflow::{
        NodePosition, WorkflowEdge, WorkflowGraph, WorkflowNode,
    };
    use serde_json::json;

    fn node(id: &str, node_type: &str, data: serde_json::Value) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            node_type: node_type.to_string(),
            position: NodePosition { x: 0.0, y: 0.0 },
            data,
        }
    }

    #[test]
    fn validate_graph_allows_logic_if_with_single_branch_edge() {
        let graph = WorkflowGraph {
            schema_version: 1,
            nodes: vec![
                node("trigger-1", "trigger.manual", json!({})),
                node(
                    "if-1",
                    "logic.if",
                    json!({
                        "rule": {
                            "field": "trigger.data.count",
                            "operator": "greaterThan",
                            "value": 0
                        }
                    }),
                ),
                node("agent-1", "agent.run", json!({})),
            ],
            edges: vec![
                WorkflowEdge {
                    id: "edge-trigger-if".to_string(),
                    source: "trigger-1".to_string(),
                    target: "if-1".to_string(),
                    source_handle: None,
                    target_handle: None,
                },
                WorkflowEdge {
                    id: "edge-if-true".to_string(),
                    source: "if-1".to_string(),
                    target: "agent-1".to_string(),
                    source_handle: Some("true".to_string()),
                    target_handle: None,
                },
            ],
        };

        assert!(validate_graph(&graph).is_ok());
    }

    #[test]
    fn workflow_node_default_data_includes_code_nodes() {
        assert_eq!(
            workflow_node_default_data("code.bash.run"),
            Some(json!({
                "script": "",
                "workingDirectory": ".",
                "timeoutSeconds": 120,
            }))
        );
        assert_eq!(
            workflow_node_default_data("code.script.run"),
            Some(json!({
                "language": "typescript",
                "source": "",
                "workingDirectory": ".",
                "timeoutSeconds": 120,
            }))
        );
    }

    #[test]
    fn workflow_runtime_warnings_ignore_code_nodes_with_executors() {
        let graph = WorkflowGraph {
            schema_version: 1,
            nodes: vec![
                node("trigger-1", "trigger.manual", json!({})),
                node("code-1", "code.bash.run", json!({})),
                node("code-2", "code.script.run", json!({})),
            ],
            edges: vec![],
        };

        assert!(workflow_runtime_warnings(&graph).is_empty());
    }
}
