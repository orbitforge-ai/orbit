use crate::db::cloud::{CloudClientState, SupabaseClient};
use crate::db::DbPool;
use crate::models::project_workflow::{
    CreateProjectWorkflow, ProjectWorkflow, RuleNode, UpdateProjectWorkflow, WorkflowEdge,
    WorkflowGraph, WorkflowNode, KNOWN_NODE_TYPES, RULE_OPERATORS,
};
use crate::models::schedule::RecurringConfig;
use crate::scheduler::converter::next_n_runs;
use crate::workflows::nodes::node_type_has_executor;
use rusqlite::params;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use ulid::Ulid;

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

// ── Row mapper ────────────────────────────────────────────────────────────────

const COLUMNS: &str = "id, project_id, name, description, enabled, graph,
        trigger_kind, trigger_config, version, created_at, updated_at";

pub(crate) fn map_workflow(row: &rusqlite::Row) -> rusqlite::Result<ProjectWorkflow> {
    let enabled: i64 = row.get(4)?;
    let graph_json: String = row.get(5)?;
    let trigger_config_json: String = row.get(7)?;
    Ok(ProjectWorkflow {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        enabled: enabled != 0,
        graph: serde_json::from_str(&graph_json).unwrap_or_default(),
        trigger_kind: row.get(6)?,
        trigger_config: serde_json::from_str(&trigger_config_json)
            .unwrap_or_else(|_| serde_json::json!({})),
        version: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
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

fn derive_trigger_from_graph(
    graph: &WorkflowGraph,
    fallback_kind: Option<&str>,
    fallback_config: Option<&serde_json::Value>,
) -> (String, serde_json::Value) {
    if let Some(node) = graph
        .nodes
        .iter()
        .find(|node| node.node_type == "trigger.schedule")
    {
        return ("schedule".to_string(), node.data.clone());
    }
    if graph
        .nodes
        .iter()
        .any(|node| node.node_type == "trigger.manual")
    {
        return ("manual".to_string(), serde_json::json!({}));
    }
    (
        fallback_kind.unwrap_or("manual").to_string(),
        fallback_config
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    )
}

fn sync_workflow_schedule(
    conn: &rusqlite::Connection,
    workflow_id: &str,
    enabled: bool,
    trigger_kind: &str,
    trigger_config: &serde_json::Value,
    now: &str,
) -> Result<(), String> {
    let schedule_id = format!("workflow-schedule-{}", workflow_id);
    if !enabled || trigger_kind != "schedule" {
        conn.execute(
            "DELETE FROM schedules WHERE workflow_id = ?1",
            params![workflow_id],
        )
        .map_err(|e| e.to_string())?;
        return Ok(());
    }

    let config: RecurringConfig = serde_json::from_value(trigger_config.clone())
        .map_err(|e| format!("workflow schedule config is invalid: {}", e))?;
    let next_run_at = next_n_runs(&config, 1).into_iter().next();
    let config_json = serde_json::to_string(trigger_config).map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT OR REPLACE INTO schedules (
            id, task_id, workflow_id, target_kind, kind, config, enabled,
            next_run_at, last_run_at, created_at, updated_at
         ) VALUES (?1, NULL, ?2, 'workflow', 'recurring', ?3, 1, ?4, NULL,
                   COALESCE((SELECT created_at FROM schedules WHERE id = ?1), ?5), ?5)",
        params![schedule_id, workflow_id, config_json, next_run_at, now],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
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

fn prepare_graph_for_write(
    graph: &WorkflowGraph,
    fallback_kind: Option<&str>,
    fallback_config: Option<&Value>,
) -> Result<(WorkflowGraph, String, Value), String> {
    let normalized = normalize_graph_for_storage(graph);
    validate_graph(&normalized)?;
    let (trigger_kind, trigger_config) =
        derive_trigger_from_graph(&normalized, fallback_kind, fallback_config);
    Ok((normalized, trigger_kind, trigger_config))
}

fn structured_workflow_error(code: &str, message: String) -> String {
    json!({
        "code": code,
        "message": message,
    })
    .to_string()
}

fn ensure_workflow_can_enable_or_run(graph: &WorkflowGraph, action: &str) -> Result<(), String> {
    if workflow_has_trigger_node(graph) {
        return Ok(());
    }
    Err(structured_workflow_error(
        "workflow_missing_trigger",
        format!("workflow cannot {} without a trigger node", action),
    ))
}

fn spawn_cloud_upsert_workflow(cloud: Option<Arc<SupabaseClient>>, workflow: &ProjectWorkflow) {
    if let Some(client) = cloud {
        let workflow = workflow.clone();
        tokio::spawn(async move {
            if let Err(err) = client.upsert_project_workflow(&workflow).await {
                tracing::warn!("cloud upsert project_workflow: {}", err);
            }
        });
    }
}

fn spawn_cloud_delete_workflow(cloud: Option<Arc<SupabaseClient>>, workflow_id: &str) {
    if let Some(client) = cloud {
        let workflow_id = workflow_id.to_string();
        tokio::spawn(async move {
            if let Err(err) = client.delete_by_id("project_workflows", &workflow_id).await {
                tracing::warn!("cloud delete project_workflows: {}", err);
            }
        });
    }
}

pub async fn list_project_workflows_with_db(
    db: &DbPool,
    project_id: &str,
    limit: Option<i64>,
) -> Result<Vec<ProjectWorkflow>, String> {
    let pool = db.0.clone();
    let project_id = project_id.to_string();
    let limit = limit.unwrap_or(100).clamp(1, 200);
    tokio::task::spawn_blocking(move || -> Result<Vec<ProjectWorkflow>, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let sql = format!(
            "SELECT {} FROM project_workflows WHERE project_id = ?1 ORDER BY name ASC LIMIT ?2",
            COLUMNS
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![project_id, limit], map_workflow)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn get_project_workflow_with_db(
    db: &DbPool,
    id: &str,
) -> Result<ProjectWorkflow, String> {
    let pool = db.0.clone();
    let id = id.to_string();
    tokio::task::spawn_blocking(move || -> Result<ProjectWorkflow, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let sql = format!("SELECT {} FROM project_workflows WHERE id = ?1", COLUMNS);
        conn.query_row(&sql, params![id], map_workflow)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn lookup_workflow_project_id_with_db(
    db: &DbPool,
    workflow_id: &str,
) -> Result<String, String> {
    let pool = db.0.clone();
    let workflow_id = workflow_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<String, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT project_id FROM project_workflows WHERE id = ?1",
            params![workflow_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("workflow: not found ({})", e))
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn lookup_run_scope_with_db(
    db: &DbPool,
    run_id: &str,
) -> Result<(String, String), String> {
    let pool = db.0.clone();
    let run_id = run_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<(String, String), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT wr.workflow_id, pw.project_id
             FROM workflow_runs wr
             INNER JOIN project_workflows pw ON pw.id = wr.workflow_id
             WHERE wr.id = ?1",
            params![run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| format!("workflow run not found ({})", e))
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn create_project_workflow_with_db(
    db: &DbPool,
    cloud: Option<Arc<SupabaseClient>>,
    payload: CreateProjectWorkflow,
) -> Result<ProjectWorkflow, String> {
    let pool = db.0.clone();
    let item = tokio::task::spawn_blocking(move || -> Result<ProjectWorkflow, String> {
        let mut conn = pool.get().map_err(|e| e.to_string())?;
        let graph = payload.graph.unwrap_or_default();
        let (graph, trigger_kind, trigger_config) = prepare_graph_for_write(
            &graph,
            payload.trigger_kind.as_deref(),
            payload.trigger_config.as_ref(),
        )?;

        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let id = Ulid::new().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let graph_json = serde_json::to_string(&graph).map_err(|e| e.to_string())?;
        let trigger_config_json =
            serde_json::to_string(&trigger_config).map_err(|e| e.to_string())?;

        tx.execute(
            "INSERT INTO project_workflows (
                id, project_id, name, description, enabled, graph,
                trigger_kind, trigger_config, version, created_at, updated_at
             ) VALUES (?1,?2,?3,?4,0,?5,?6,?7,1,?8,?8)",
            params![
                id,
                payload.project_id,
                payload.name,
                payload.description,
                graph_json,
                trigger_kind,
                trigger_config_json,
                now,
            ],
        )
        .map_err(|e| e.to_string())?;

        sync_workflow_schedule(&tx, &id, false, &trigger_kind, &trigger_config, &now)?;

        let sql = format!("SELECT {} FROM project_workflows WHERE id = ?1", COLUMNS);
        let item = tx
            .query_row(&sql, params![id], map_workflow)
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(item)
    })
    .await
    .map_err(|e| e.to_string())??;

    spawn_cloud_upsert_workflow(cloud, &item);
    Ok(item)
}

pub async fn update_project_workflow_with_db(
    db: &DbPool,
    cloud: Option<Arc<SupabaseClient>>,
    id: &str,
    payload: UpdateProjectWorkflow,
) -> Result<ProjectWorkflow, String> {
    let pool = db.0.clone();
    let id = id.to_string();
    let item = tokio::task::spawn_blocking(move || -> Result<ProjectWorkflow, String> {
        let mut conn = pool.get().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();

        let current = tx
            .query_row(
                &format!("SELECT {} FROM project_workflows WHERE id = ?1", COLUMNS),
                params![id],
                map_workflow,
            )
            .map_err(|e| e.to_string())?;

        if let Some(name) = &payload.name {
            if name.trim().is_empty() {
                return Err("workflow: name must be non-empty".into());
            }
            tx.execute(
                "UPDATE project_workflows SET name = ?1, updated_at = ?2 WHERE id = ?3",
                params![name, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(description) = &payload.description {
            tx.execute(
                "UPDATE project_workflows SET description = ?1, updated_at = ?2 WHERE id = ?3",
                params![description, now, id],
            )
            .map_err(|e| e.to_string())?;
        }

        let normalized_graph = if let Some(graph) = &payload.graph {
            let (graph, _, _) = prepare_graph_for_write(
                graph,
                payload.trigger_kind.as_deref().or(Some(current.trigger_kind.as_str())),
                payload.trigger_config.as_ref().or(Some(&current.trigger_config)),
            )?;
            let json = serde_json::to_string(&graph).map_err(|e| e.to_string())?;
            tx.execute(
                "UPDATE project_workflows
                    SET graph = ?1, version = version + 1, updated_at = ?2
                  WHERE id = ?3",
                params![json, now, id],
            )
            .map_err(|e| e.to_string())?;
            Some(graph)
        } else {
            None
        };

        let graph_for_trigger = normalized_graph.as_ref().unwrap_or(&current.graph);
        let (trigger_kind, trigger_config) = derive_trigger_from_graph(
            graph_for_trigger,
            payload.trigger_kind.as_deref().or(Some(current.trigger_kind.as_str())),
            payload.trigger_config.as_ref().or(Some(&current.trigger_config)),
        );
        let trigger_config_json =
            serde_json::to_string(&trigger_config).map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE project_workflows SET trigger_kind = ?1, trigger_config = ?2, updated_at = ?3 WHERE id = ?4",
            params![trigger_kind, trigger_config_json, now, id],
        )
        .map_err(|e| e.to_string())?;
        sync_workflow_schedule(&tx, &current.id, current.enabled, &trigger_kind, &trigger_config, &now)?;

        let item = tx
            .query_row(
                &format!("SELECT {} FROM project_workflows WHERE id = ?1", COLUMNS),
                params![id],
                map_workflow,
            )
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(item)
    })
    .await
    .map_err(|e| e.to_string())??;

    spawn_cloud_upsert_workflow(cloud, &item);
    Ok(item)
}

pub async fn delete_project_workflow_with_db(
    db: &DbPool,
    cloud: Option<Arc<SupabaseClient>>,
    id: &str,
) -> Result<(), String> {
    let pool = db.0.clone();
    let workflow_id = id.to_string();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut conn = pool.get().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM schedules WHERE workflow_id = ?1",
            params![workflow_id.clone()],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM project_workflows WHERE id = ?1",
            params![workflow_id],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;

    spawn_cloud_delete_workflow(cloud, id);
    Ok(())
}

pub async fn set_project_workflow_enabled_with_db(
    db: &DbPool,
    cloud: Option<Arc<SupabaseClient>>,
    id: &str,
    enabled: bool,
) -> Result<ProjectWorkflow, String> {
    let pool = db.0.clone();
    let id = id.to_string();
    let item = tokio::task::spawn_blocking(move || -> Result<ProjectWorkflow, String> {
        let mut conn = pool.get().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let current = tx
            .query_row(
                &format!("SELECT {} FROM project_workflows WHERE id = ?1", COLUMNS),
                params![id],
                map_workflow,
            )
            .map_err(|e| e.to_string())?;
        if enabled {
            ensure_workflow_can_enable_or_run(&current.graph, "enable")?;
        }

        let now = chrono::Utc::now().to_rfc3339();
        let flag: i64 = if enabled { 1 } else { 0 };
        tx.execute(
            "UPDATE project_workflows SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
            params![flag, now, id],
        )
        .map_err(|e| e.to_string())?;
        sync_workflow_schedule(
            &tx,
            &current.id,
            enabled,
            &current.trigger_kind,
            &current.trigger_config,
            &now,
        )?;
        let item = tx
            .query_row(
                &format!("SELECT {} FROM project_workflows WHERE id = ?1", COLUMNS),
                params![id],
                map_workflow,
            )
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(item)
    })
    .await
    .map_err(|e| e.to_string())??;

    spawn_cloud_upsert_workflow(cloud, &item);
    Ok(item)
}

// ── Commands ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_project_workflows(
    project_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<ProjectWorkflow>, String> {
    list_project_workflows_with_db(db.inner(), &project_id, None).await
}

#[tauri::command]
pub async fn get_project_workflow(
    id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<ProjectWorkflow, String> {
    get_project_workflow_with_db(db.inner(), &id).await
}

#[tauri::command]
pub async fn create_project_workflow(
    payload: CreateProjectWorkflow,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<ProjectWorkflow, String> {
    create_project_workflow_with_db(db.inner(), cloud.get(), payload).await
}

#[tauri::command]
pub async fn update_project_workflow(
    id: String,
    payload: UpdateProjectWorkflow,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<ProjectWorkflow, String> {
    update_project_workflow_with_db(db.inner(), cloud.get(), &id, payload).await
}

#[tauri::command]
pub async fn delete_project_workflow(
    id: String,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
    delete_project_workflow_with_db(db.inner(), cloud.get(), &id).await
}

#[tauri::command]
pub async fn set_project_workflow_enabled(
    id: String,
    enabled: bool,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<ProjectWorkflow, String> {
    set_project_workflow_enabled_with_db(db.inner(), cloud.get(), &id, enabled).await
}

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
