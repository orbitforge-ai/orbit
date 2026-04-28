-- Migration 9: Remove legacy sessions table and its FK from tasks
--
-- The old `sessions` table was superseded by `chat_sessions` (migration 3).
-- tasks.session_id referenced sessions(id) but was never populated.
-- Rebuild the tasks table without that FK so the dead table can be dropped.

-- Must disable FK enforcement for the table-rebuild trick
PRAGMA foreign_keys=OFF;

CREATE TABLE tasks_new (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT,
  kind TEXT NOT NULL,
  config TEXT NOT NULL,
  max_duration_seconds INTEGER NOT NULL DEFAULT 3600,
  max_retries INTEGER NOT NULL DEFAULT 0,
  retry_delay_seconds INTEGER NOT NULL DEFAULT 60,
  concurrency_policy TEXT NOT NULL DEFAULT 'allow',
  tags TEXT NOT NULL DEFAULT '[]',
  agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

INSERT INTO tasks_new (id, name, description, kind, config, max_duration_seconds,
  max_retries, retry_delay_seconds, concurrency_policy, tags, agent_id, enabled,
  created_at, updated_at)
SELECT id, name, description, kind, config, max_duration_seconds,
  max_retries, retry_delay_seconds, concurrency_policy, tags, agent_id, enabled,
  created_at, updated_at
FROM tasks;

DROP TABLE tasks;
ALTER TABLE tasks_new RENAME TO tasks;

CREATE INDEX IF NOT EXISTS idx_tasks_agent_id ON tasks(agent_id);
CREATE INDEX IF NOT EXISTS idx_tasks_enabled ON tasks(enabled);

-- Now safe to remove the legacy table
DROP TABLE IF EXISTS sessions;

PRAGMA foreign_keys=ON;
