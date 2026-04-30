use serde::{Deserialize, Serialize};

/// Top-level chat session, including the joined-source-bus-message metadata
/// the inbox UI uses to render "from agent X via Y" subtitles.
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

/// A chat message as stored on disk. The content payload is kept as the raw
/// JSON string the row holds so the repo trait stays free of dependencies on
/// `executor::llm_provider::ContentBlock`. The command layer parses it into
/// the typed shape the UI expects.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageRow {
    pub id: String,
    pub role: String,
    pub content_json: String,
    pub created_at: Option<String>,
    pub is_compacted: bool,
}

/// Pagination wrapper around `ChatMessageRow`. Mirrors the legacy
/// `PaginatedChatMessages` DTO at the row level.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageRows {
    pub messages: Vec<ChatMessageRow>,
    pub total_count: i64,
    pub has_more: bool,
}

/// Lightweight session-info DTO joined with the project name — used by the
/// chat header bar and tool dispatch context.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSessionMeta {
    pub session_id: String,
    pub agent_id: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
}

/// Snapshot of a session's execution lifecycle (running / cancelled /
/// finished + summary + error). Polled from the UI to drive the chat
/// "thinking…" / "cancelled" indicator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionExecutionStatus {
    pub session_id: String,
    pub execution_state: Option<String>,
    pub finish_summary: Option<String>,
    pub terminal_error: Option<String>,
}

/// A single emoji reaction on a message. Keyed by `(message_id, emoji)`
/// uniqueness in the underlying table.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageReactionRow {
    pub id: String,
    pub message_id: String,
    pub emoji: String,
    pub created_at: String,
}

/// Last-input-tokens + agent_id needed to compute remaining context window
/// for a session. Pulled together so the UI can show "X% of context used".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSessionTokenUsage {
    pub last_input_tokens: Option<u32>,
    pub agent_id: String,
}
