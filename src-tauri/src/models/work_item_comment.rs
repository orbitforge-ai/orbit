use serde::{Deserialize, Serialize};

/// A comment on a work item. Author may be the workspace user or an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemComment {
    pub id: String,
    pub work_item_id: String,
    /// `user` | `agent`
    pub author_kind: String,
    /// Populated only when `author_kind = "agent"`.
    pub author_agent_id: Option<String>,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum CommentAuthor {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "agent")]
    Agent { agent_id: String },
}
