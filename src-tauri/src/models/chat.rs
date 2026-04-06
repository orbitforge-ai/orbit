use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSession {
    pub id: String,
    pub agent_id: String,
    pub title: String,
    pub archived: bool,
    pub session_type: String,
    pub parent_session_id: Option<String>,
    pub source_bus_message_id: Option<String>,
    pub chain_depth: i64,
    pub execution_state: Option<String>,
    pub finish_summary: Option<String>,
    pub terminal_error: Option<String>,
    pub source_agent_id: Option<String>,
    pub source_agent_name: Option<String>,
    pub source_session_id: Option<String>,
    pub source_session_title: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub project_id: Option<String>,
    pub worktree_name: Option<String>,
    pub worktree_branch: Option<String>,
    pub worktree_path: Option<String>,
}
