use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub id: String,
    pub task_id: Option<String>,
    pub workflow_id: Option<String>,
    /// 'task' | 'workflow' — distinct from `kind` (which is the cadence type).
    pub target_kind: String,
    pub kind: String,
    /// JSON-encoded config: RecurringConfig, OneShotConfig, or TriggeredConfig
    pub config: serde_json::Value,
    pub enabled: bool,
    pub next_run_at: Option<String>,
    pub last_run_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSchedule {
    pub task_id: Option<String>,
    pub workflow_id: Option<String>,
    #[serde(default)]
    pub target_kind: Option<String>,
    pub kind: String,
    pub config: serde_json::Value,
}

/// Structured recurring schedule config (converted to cron internally)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecurringConfig {
    pub interval_unit: String, // "minutes" | "hours" | "days" | "weeks" | "months"
    pub interval_value: u32,
    pub days_of_week: Option<Vec<u8>>, // 0=Sun … 6=Sat
    pub time_of_day: Option<TimeOfDay>,
    pub timezone: String,
    pub missed_run_policy: String, // "run_once" | "skip"
    /// Original text/cron input for display in the UI
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub expression: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeOfDay {
    pub hour: u8,
    pub minute: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OneShotConfig {
    pub run_at: String, // ISO 8601
    pub timezone: String,
}
