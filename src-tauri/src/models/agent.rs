use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub state: String,
    pub max_concurrent_runs: i64,
    pub heartbeat_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgent {
    pub name: String,
    pub description: Option<String>,
    pub max_concurrent_runs: Option<i64>,
}
