use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkflow {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub graph: WorkflowGraph,
    pub trigger_kind: String,
    pub trigger_config: serde_json::Value,
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectWorkflow {
    pub project_id: String,
    pub name: String,
    pub description: Option<String>,
    pub trigger_kind: Option<String>,
    pub trigger_config: Option<serde_json::Value>,
    pub graph: Option<WorkflowGraph>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjectWorkflow {
    pub name: Option<String>,
    pub description: Option<String>,
    pub trigger_kind: Option<String>,
    pub trigger_config: Option<serde_json::Value>,
    pub graph: Option<WorkflowGraph>,
}

// ── Graph shape ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowGraph {
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
    #[serde(default = "default_schema_version")]
    pub schema_version: i64,
}

fn default_schema_version() -> i64 {
    1
}

impl Default for WorkflowGraph {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            schema_version: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowNode {
    pub id: String,
    /// e.g. `trigger.manual`, `agent.run`, `logic.if`, `integration.gmail.read`.
    #[serde(rename = "type")]
    pub node_type: String,
    pub position: NodePosition,
    #[serde(default)]
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodePosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    /// For `logic.if`: `"true"` or `"false"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_handle: Option<String>,
}

// ── Rule tree (for logic.if) ──────────────────────────────────────────────────
//
// Either a leaf rule (`field` operator `value`) or a group of rules joined
// by `combinator` (`and` | `or`). The Rust evaluator (Phase 4) walks this
// recursively. Phase 3 only persists / validates it.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum RuleNode {
    Group(RuleGroup),
    Leaf(RuleLeaf),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleGroup {
    pub combinator: String, // "and" | "or"
    pub rules: Vec<RuleNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleLeaf {
    pub field: String,
    pub operator: String,
    /// May be a literal value or `{ "field": "..." }` reference.
    #[serde(default)]
    pub value: serde_json::Value,
}

pub const RULE_OPERATORS: &[&str] = &[
    "equals",
    "notEquals",
    "contains",
    "notContains",
    "startsWith",
    "endsWith",
    "greaterThan",
    "greaterThanOrEqual",
    "lessThan",
    "lessThanOrEqual",
    "exists",
    "notExists",
    "isTrue",
    "isFalse",
    "matchesRegex",
];

pub const KNOWN_NODE_TYPES: &[&str] = &[
    "trigger.manual",
    "trigger.schedule",
    "agent.run",
    "logic.if",
    // Integration stubs — placeable but inert in Phase 3.
    "integration.gmail.read",
    "integration.gmail.send",
    "integration.slack.send",
    "integration.http.request",
];
