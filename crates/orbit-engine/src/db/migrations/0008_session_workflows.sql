-- Migration 8: session-based agentic workflows

ALTER TABLE chat_sessions ADD COLUMN session_type TEXT NOT NULL DEFAULT 'user_chat';
ALTER TABLE chat_sessions ADD COLUMN parent_session_id TEXT REFERENCES chat_sessions(id) ON DELETE SET NULL;
ALTER TABLE chat_sessions ADD COLUMN source_bus_message_id TEXT REFERENCES bus_messages(id) ON DELETE SET NULL;
ALTER TABLE chat_sessions ADD COLUMN chain_depth INTEGER NOT NULL DEFAULT 0;
ALTER TABLE chat_sessions ADD COLUMN execution_state TEXT DEFAULT NULL;
ALTER TABLE chat_sessions ADD COLUMN finish_summary TEXT DEFAULT NULL;
ALTER TABLE chat_sessions ADD COLUMN terminal_error TEXT DEFAULT NULL;

CREATE INDEX IF NOT EXISTS idx_chat_sessions_type ON chat_sessions(session_type);
CREATE INDEX IF NOT EXISTS idx_chat_sessions_execution_state ON chat_sessions(execution_state);
CREATE INDEX IF NOT EXISTS idx_chat_sessions_parent_session ON chat_sessions(parent_session_id);
CREATE INDEX IF NOT EXISTS idx_chat_sessions_source_bus_message ON chat_sessions(source_bus_message_id);

ALTER TABLE bus_messages ADD COLUMN from_session_id TEXT REFERENCES chat_sessions(id) ON DELETE SET NULL;
ALTER TABLE bus_messages ADD COLUMN to_session_id TEXT REFERENCES chat_sessions(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_bus_messages_from_session ON bus_messages(from_session_id);
CREATE INDEX IF NOT EXISTS idx_bus_messages_to_session ON bus_messages(to_session_id);

UPDATE chat_sessions
SET session_type = 'pulse'
WHERE title = 'Pulse' AND session_type = 'user_chat';

UPDATE tasks
SET enabled = 0
WHERE name LIKE 'bus:%' OR name LIKE 'sub-agent:%';
