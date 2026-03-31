use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BusMessage {
    pub id: String,
    pub from_agent_id: String,
    pub from_run_id: Option<String>,
    pub to_agent_id: String,
    pub to_run_id: Option<String>,
    pub kind: String,               // "direct" | "event"
    pub event_type: Option<String>,
    pub payload: serde_json::Value,
    pub status: String,             // "delivered" | "failed" | "depth_exceeded"
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BusSubscription {
    pub id: String,
    pub subscriber_agent_id: String,
    pub source_agent_id: String,
    pub event_type: String,         // "run:completed" | "run:failed" | "run:any_terminal"
    pub task_id: String,
    pub payload_template: String,
    pub enabled: bool,
    pub max_chain_depth: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBusSubscription {
    pub subscriber_agent_id: String,
    pub source_agent_id: String,
    pub event_type: String,
    pub task_id: String,
    #[serde(default = "default_payload_template")]
    pub payload_template: String,
    #[serde(default = "default_max_chain_depth")]
    pub max_chain_depth: i64,
}

fn default_payload_template() -> String {
    "{}".to_string()
}

fn default_max_chain_depth() -> i64 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BusThreadMessage {
    pub id: String,
    pub from_agent_id: String,
    pub from_agent_name: String,
    pub to_agent_id: String,
    pub kind: String,
    pub payload: serde_json::Value,
    pub status: String,
    pub created_at: String,
    pub triggered_run_id: Option<String>,
    pub triggered_run_state: Option<String>,
    pub triggered_run_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedBusThread {
    pub messages: Vec<BusThreadMessage>,
    pub total_count: i64,
    pub has_more: bool,
}
