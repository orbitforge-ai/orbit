use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub task_id: String,
    pub schedule_id: Option<String>,
    pub agent_id: Option<String>,
    pub state: String,
    pub trigger: String,
    pub exit_code: Option<i64>,
    pub pid: Option<i64>,
    pub log_path: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub duration_ms: Option<i64>,
    pub retry_count: i64,
    pub parent_run_id: Option<String>,
    pub metadata: serde_json::Value,
    pub is_sub_agent: bool,
    pub created_at: String,
    pub project_id: Option<String>,
}

/// All valid run states
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Pending,
    Queued,
    Running,
    Success,
    Failure,
    Cancelled,
    TimedOut,
}

impl RunState {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunState::Pending => "pending",
            RunState::Queued => "queued",
            RunState::Running => "running",
            RunState::Success => "success",
            RunState::Failure => "failure",
            RunState::Cancelled => "cancelled",
            RunState::TimedOut => "timed_out",
        }
    }
}

impl std::fmt::Display for RunState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl TryFrom<&str> for RunState {
    type Error = String;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "pending" => Ok(RunState::Pending),
            "queued" => Ok(RunState::Queued),
            "running" => Ok(RunState::Running),
            "success" => Ok(RunState::Success),
            "failure" => Ok(RunState::Failure),
            "cancelled" => Ok(RunState::Cancelled),
            "timed_out" => Ok(RunState::TimedOut),
            other => Err(format!("unknown run state: {}", other)),
        }
    }
}

/// Serialisable summary for list views (includes task name)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub id: String,
    pub task_id: String,
    pub task_name: String,
    pub schedule_id: Option<String>,
    pub agent_id: Option<String>,
    pub agent_name: Option<String>,
    pub state: String,
    pub trigger: String,
    pub exit_code: Option<i64>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub duration_ms: Option<i64>,
    pub retry_count: i64,
    pub is_sub_agent: bool,
    pub created_at: String,
    pub chat_session_id: Option<String>,
    pub project_id: Option<String>,
}
