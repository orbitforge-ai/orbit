-- Migration 13: Compaction improvements
-- 1. Rebuild chat_compaction_summaries to make original_token_count nullable
-- 2. Add circuit breaker fields to chat_sessions

-- Step 1: Rebuild chat_compaction_summaries with nullable original_token_count
-- SQLite cannot ALTER COLUMN from NOT NULL to nullable, so we must rebuild.

CREATE TABLE chat_compaction_summaries_new (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    summary_message_id TEXT NOT NULL REFERENCES chat_messages(id) ON DELETE CASCADE,
    compacted_message_ids TEXT NOT NULL,
    original_token_count INTEGER DEFAULT NULL,
    summary_token_count INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

INSERT INTO chat_compaction_summaries_new (id, session_id, summary_message_id, compacted_message_ids, original_token_count, summary_token_count, created_at)
    SELECT id, session_id, summary_message_id, compacted_message_ids, original_token_count, summary_token_count, created_at
    FROM chat_compaction_summaries;

DROP TABLE chat_compaction_summaries;
ALTER TABLE chat_compaction_summaries_new RENAME TO chat_compaction_summaries;

CREATE INDEX idx_compaction_summaries_session ON chat_compaction_summaries(session_id);

-- Step 2: Add circuit breaker fields to chat_sessions
ALTER TABLE chat_sessions ADD COLUMN compaction_failure_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE chat_sessions ADD COLUMN compaction_last_failure_at TEXT DEFAULT NULL;
