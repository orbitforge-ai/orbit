-- Migration 6: Agent Bus (inter-agent messaging and event subscriptions)

-- Audit log of all inter-agent messages
CREATE TABLE IF NOT EXISTS bus_messages (
  id            TEXT PRIMARY KEY,
  from_agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  from_run_id   TEXT REFERENCES runs(id) ON DELETE SET NULL,
  to_agent_id   TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  to_run_id     TEXT REFERENCES runs(id) ON DELETE SET NULL,
  kind          TEXT NOT NULL DEFAULT 'direct',   -- 'direct' | 'event'
  event_type    TEXT,                              -- e.g. 'run:completed', 'run:failed'
  payload       TEXT NOT NULL DEFAULT '{}',
  status        TEXT NOT NULL DEFAULT 'delivered', -- 'delivered' | 'failed' | 'depth_exceeded'
  created_at    TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_bus_messages_from_agent ON bus_messages(from_agent_id);
CREATE INDEX IF NOT EXISTS idx_bus_messages_to_agent   ON bus_messages(to_agent_id);
CREATE INDEX IF NOT EXISTS idx_bus_messages_created_at ON bus_messages(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_bus_messages_from_run   ON bus_messages(from_run_id);

-- Event subscriptions: agent subscribes to events from another agent
CREATE TABLE IF NOT EXISTS bus_subscriptions (
  id                  TEXT PRIMARY KEY,
  subscriber_agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  source_agent_id     TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  event_type          TEXT NOT NULL,   -- 'run:completed' | 'run:failed' | 'run:any_terminal'
  task_id             TEXT REFERENCES tasks(id) ON DELETE CASCADE,
  payload_template    TEXT NOT NULL DEFAULT '{}',
  enabled             INTEGER NOT NULL DEFAULT 1,
  max_chain_depth     INTEGER NOT NULL DEFAULT 10,
  created_at          TEXT NOT NULL,
  updated_at          TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_bus_subs_source     ON bus_subscriptions(source_agent_id, event_type);
CREATE INDEX IF NOT EXISTS idx_bus_subs_subscriber ON bus_subscriptions(subscriber_agent_id);

-- Add chain tracking to runs
ALTER TABLE runs ADD COLUMN chain_depth           INTEGER NOT NULL DEFAULT 0;
ALTER TABLE runs ADD COLUMN source_bus_message_id TEXT REFERENCES bus_messages(id);
