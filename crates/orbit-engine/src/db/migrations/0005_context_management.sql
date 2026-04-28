-- Migration 5: Context window management for chat sessions
-- Adds token tracking, compaction support, and context usage tracking

ALTER TABLE chat_messages ADD COLUMN token_count INTEGER DEFAULT NULL;
ALTER TABLE chat_messages ADD COLUMN is_compacted INTEGER NOT NULL DEFAULT 0;
ALTER TABLE chat_sessions ADD COLUMN last_input_tokens INTEGER DEFAULT NULL;

CREATE TABLE chat_compaction_summaries (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    summary_message_id TEXT NOT NULL REFERENCES chat_messages(id) ON DELETE CASCADE,
    compacted_message_ids TEXT NOT NULL,
    original_token_count INTEGER NOT NULL,
    summary_token_count INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_compaction_summaries_session ON chat_compaction_summaries(session_id);
