use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSession {
    pub id: String,
    pub agent_id: String,
    pub title: String,
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
}
