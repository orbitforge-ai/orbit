use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTask {
    pub id: String,
    pub session_id: String,
    pub agent_id: String,
    pub subject: String,
    pub description: Option<String>,
    pub status: String,
    pub active_form: Option<String>,
    pub blocked_by: Vec<String>,
    pub metadata: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}
