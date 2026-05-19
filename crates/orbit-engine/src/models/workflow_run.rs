use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRun {
    pub id: String,
    pub workflow_id: String,
    pub workflow_version: i64,
    pub graph_snapshot: serde_json::Value,
    pub trigger_kind: String,
    pub trigger_data: serde_json::Value,
    pub status: String,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunSummary {
    pub id: String,
    pub workflow_id: String,
    pub workflow_name: String,
    pub workflow_version: i64,
    pub trigger_kind: String,
    pub status: String,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunStep {
    pub id: String,
    pub run_id: String,
    pub node_id: String,
    pub node_type: String,
    pub status: String,
    pub input: serde_json::Value,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub sequence: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunWithSteps {
    #[serde(flatten)]
    pub run: WorkflowRun,
    pub steps: Vec<WorkflowRunStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunAgentSession {
    pub node_id: String,
    pub session_id: String,
    pub agent_id: String,
    pub execution_state: Option<String>,
    pub finish_summary: Option<String>,
    pub terminal_error: Option<String>,
    pub messages: Vec<WorkflowRunChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunChatMessage {
    pub id: Option<String>,
    pub role: String,
    pub content: Vec<crate::executor::llm_provider::ContentBlock>,
    pub created_at: Option<String>,
    #[serde(rename = "isCompacted")]
    pub is_compacted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunView {
    #[serde(flatten)]
    pub run: WorkflowRun,
    pub steps: Vec<WorkflowRunStep>,
    pub agent_sessions: Vec<WorkflowRunAgentSession>,
}
