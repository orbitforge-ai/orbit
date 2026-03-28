use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub kind: String,
    /// JSON-encoded config blob (ShellCommandConfig, HttpRequestConfig, etc.)
    pub config: serde_json::Value,
    pub max_duration_seconds: i64,
    pub max_retries: i64,
    pub retry_delay_seconds: i64,
    pub concurrency_policy: String,
    pub tags: Vec<String>,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTask {
    pub name: String,
    pub description: Option<String>,
    pub kind: String,
    pub config: serde_json::Value,
    pub max_duration_seconds: Option<i64>,
    pub max_retries: Option<i64>,
    pub retry_delay_seconds: Option<i64>,
    pub concurrency_policy: Option<String>,
    pub tags: Option<Vec<String>>,
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTask {
    pub name: Option<String>,
    pub description: Option<String>,
    pub config: Option<serde_json::Value>,
    pub max_duration_seconds: Option<i64>,
    pub max_retries: Option<i64>,
    pub retry_delay_seconds: Option<i64>,
    pub concurrency_policy: Option<String>,
    pub tags: Option<Vec<String>>,
    pub agent_id: Option<String>,
    pub enabled: Option<bool>,
}

/// Typed shell command config (stored as JSON in tasks.config)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellCommandConfig {
    pub command: String,
    pub working_directory: Option<String>,
    pub environment: Option<std::collections::HashMap<String, String>>,
    pub shell: Option<String>,
}

/// Script file config — executes a file on disk
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptFileConfig {
    pub script_path: String,
    pub interpreter: Option<String>, // e.g. "/bin/bash", "/usr/bin/python3"
    pub working_directory: Option<String>,
    pub environment: Option<std::collections::HashMap<String, String>>,
}

/// HTTP request config
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequestConfig {
    pub url: String,
    pub method: String, // GET, POST, PUT, PATCH, DELETE
    pub headers: Option<std::collections::HashMap<String, String>>,
    pub body: Option<String>,
    pub timeout_seconds: Option<u64>,
    pub expected_status_codes: Option<Vec<u16>>,
}

/// Agent step config — runs a shell command in the context of a specific agent
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStepConfig {
    pub command: String,
    pub working_directory: Option<String>,
    pub environment: Option<std::collections::HashMap<String, String>>,
    pub session_id: Option<String>,
}
