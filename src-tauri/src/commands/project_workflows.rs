use crate::db::cloud::CloudClientState;
use crate::db::DbPool;
use crate::models::project_workflow::{
    CreateProjectWorkflow, ProjectWorkflow, RuleNode, UpdateProjectWorkflow, WorkflowGraph,
    KNOWN_NODE_TYPES, RULE_OPERATORS,
};
use rusqlite::params;
use std::collections::{HashMap, HashSet};
use ulid::Ulid;

// ── Cloud helper ──────────────────────────────────────────────────────────────

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
// - `logic.if` nodes must have exactly two outgoing edges (sourceHandle
//   "true" and "false"), and any rule tree they carry must use only known
//   operators and combinators.
// - No fan-in: every non-trigger node may have at most one incoming edge.

pub fn validate_graph(graph: &WorkflowGraph) -> Result<(), String> {
    let node_ids: HashSet<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    if node_ids.len() != graph.nodes.len() {
        return Err("workflow: duplicate node ids".into());
    }

    for node in &graph.nodes {
        if !KNOWN_NODE_TYPES.contains(&node.node_type.as_str()) {
            return Err(format!(
                "workflow: unknown node type '{}' on node '{}'",
                node.node_type, node.id
            ));
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
            logic_if_handles
                .entry(edge.source.as_str())
                .or_default()
                .insert(handle);
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

    // Each logic.if must have exactly two distinct outgoing handles when it
    // has any outgoing edges. (Saving with zero outgoing is OK during early
    // editing; the runtime will treat that as a terminal branch.)
    for id in &logic_if_ids {
        if let Some(handles) = logic_if_handles.get(id) {
            if handles.len() != 2 {
                return Err(format!(
                    "workflow: logic.if node '{}' must have exactly two outgoing edges (true and false); found handles: {:?}",
                    id, handles
                ));
            }
        }
    }

    Ok(())
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

// ── Commands ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_project_workflows(
    project_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<ProjectWorkflow>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let sql = format!(
            "SELECT {} FROM project_workflows WHERE project_id = ?1 ORDER BY name ASC",
            COLUMNS
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![project_id], map_workflow)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_project_workflow(
    id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<ProjectWorkflow, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let sql = format!("SELECT {} FROM project_workflows WHERE id = ?1", COLUMNS);
        conn.query_row(&sql, params![id], map_workflow)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn create_project_workflow(
    payload: CreateProjectWorkflow,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<ProjectWorkflow, String> {
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();

    // Validate before write.
    let graph = payload.graph.clone().unwrap_or_default();
    validate_graph(&graph)?;

    let item = tokio::task::spawn_blocking(move || -> Result<ProjectWorkflow, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let id = Ulid::new().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let trigger_kind = payload.trigger_kind.unwrap_or_else(|| "manual".to_string());
        let trigger_config = payload
            .trigger_config
            .unwrap_or_else(|| serde_json::json!({}));
        let graph_json = serde_json::to_string(&graph).map_err(|e| e.to_string())?;
        let trigger_config_json =
            serde_json::to_string(&trigger_config).map_err(|e| e.to_string())?;

        conn.execute(
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

        let sql = format!("SELECT {} FROM project_workflows WHERE id = ?1", COLUMNS);
        conn.query_row(&sql, params![id], map_workflow)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_upsert_workflow!(cloud, item);
    Ok(item)
}

#[tauri::command]
pub async fn update_project_workflow(
    id: String,
    payload: UpdateProjectWorkflow,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<ProjectWorkflow, String> {
    if let Some(graph) = &payload.graph {
        validate_graph(graph)?;
    }
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let item = tokio::task::spawn_blocking(move || -> Result<ProjectWorkflow, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();

        if let Some(name) = &payload.name {
            if name.trim().is_empty() {
                return Err("workflow: name must be non-empty".into());
            }
            conn.execute(
                "UPDATE project_workflows SET name = ?1, updated_at = ?2 WHERE id = ?3",
                params![name, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(description) = &payload.description {
            conn.execute(
                "UPDATE project_workflows SET description = ?1, updated_at = ?2 WHERE id = ?3",
                params![description, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(trigger_kind) = &payload.trigger_kind {
            conn.execute(
                "UPDATE project_workflows SET trigger_kind = ?1, updated_at = ?2 WHERE id = ?3",
                params![trigger_kind, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(trigger_config) = &payload.trigger_config {
            let json = serde_json::to_string(trigger_config).map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE project_workflows SET trigger_config = ?1, updated_at = ?2 WHERE id = ?3",
                params![json, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(graph) = &payload.graph {
            let json = serde_json::to_string(graph).map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE project_workflows
                    SET graph = ?1, version = version + 1, updated_at = ?2
                  WHERE id = ?3",
                params![json, now, id],
            )
            .map_err(|e| e.to_string())?;
        }

        let sql = format!("SELECT {} FROM project_workflows WHERE id = ?1", COLUMNS);
        conn.query_row(&sql, params![id], map_workflow)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_upsert_workflow!(cloud, item);
    Ok(item)
}

#[tauri::command]
pub async fn delete_project_workflow(
    id: String,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let id_clone = id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM project_workflows WHERE id = ?1",
            params![id_clone],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_delete!(cloud, "project_workflows", id);
    Ok(())
}

#[tauri::command]
pub async fn set_project_workflow_enabled(
    id: String,
    enabled: bool,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<ProjectWorkflow, String> {
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let item = tokio::task::spawn_blocking(move || -> Result<ProjectWorkflow, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        let flag: i64 = if enabled { 1 } else { 0 };
        conn.execute(
            "UPDATE project_workflows SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
            params![flag, now, id],
        )
        .map_err(|e| e.to_string())?;
        let sql = format!("SELECT {} FROM project_workflows WHERE id = ?1", COLUMNS);
        conn.query_row(&sql, params![id], map_workflow)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_upsert_workflow!(cloud, item);
    Ok(item)
}
