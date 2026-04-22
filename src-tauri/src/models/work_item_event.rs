use serde::{Deserialize, Serialize};

/// An append-only audit record of changes to a work item. Feeds the Activity
/// tab of the work-item modal. One row per field change (update commands emit
/// one event per changed field) or lifecycle event (create/move/block/complete,
/// comment add/edit/delete).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemEvent {
    pub id: String,
    pub work_item_id: String,
    /// `user` | `agent` | `system`
    pub actor_kind: String,
    pub actor_agent_id: Option<String>,
    /// See `work_item_events::event_kind` for the canonical enum.
    pub kind: String,
    /// Arbitrary JSON payload (diff from/to, comment id, reason, etc.).
    pub payload: serde_json::Value,
    pub created_at: String,
}
