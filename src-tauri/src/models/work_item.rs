use serde::{Deserialize, Serialize};

/// A project work item — persistent kanban board card. Distinct from scheduled
/// `Task` (executable job spec) and session-local `AgentTask` (scratch-pad
/// TODO). Manipulated by humans or agents via the board UI and the `work_item`
/// agent tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItem {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub description: Option<String>,
    /// `task` | `bug` | `story` | `spike` | `chore`
    pub kind: String,
    pub column_id: Option<String>,
    /// `backlog` | `todo` | `in_progress` | `blocked` | `review` | `done` | `cancelled`
    pub status: String,
    /// 0..3 (low..urgent)
    pub priority: i64,
    pub assignee_agent_id: Option<String>,
    pub created_by_agent_id: Option<String>,
    pub parent_work_item_id: Option<String>,
    /// Float ordering within a column; gaps allow cheap reorder.
    pub position: f64,
    pub labels: Vec<String>,
    pub metadata: serde_json::Value,
    pub blocked_reason: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkItem {
    pub project_id: String,
    pub title: String,
    pub description: Option<String>,
    pub kind: Option<String>,
    pub column_id: Option<String>,
    pub status: Option<String>,
    pub priority: Option<i64>,
    pub assignee_agent_id: Option<String>,
    pub created_by_agent_id: Option<String>,
    pub parent_work_item_id: Option<String>,
    pub position: Option<f64>,
    pub labels: Option<Vec<String>>,
    pub metadata: Option<serde_json::Value>,
}

/// Update payload. `None` on a field means "don't modify it".
/// To clear an optional field, use the dedicated helper commands
/// (e.g. `unassign_work_item`) or pass an empty string where the
/// command documents that behavior.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWorkItem {
    pub title: Option<String>,
    pub description: Option<String>,
    pub kind: Option<String>,
    pub column_id: Option<String>,
    pub priority: Option<i64>,
    pub labels: Option<Vec<String>>,
    pub metadata: Option<serde_json::Value>,
}
