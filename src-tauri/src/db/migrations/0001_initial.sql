PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;

CREATE TABLE IF NOT EXISTS agents (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT,
  state TEXT NOT NULL DEFAULT 'idle',
  max_concurrent_runs INTEGER NOT NULL DEFAULT 5,
  heartbeat_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT,
  environment TEXT NOT NULL DEFAULT '{}',
  tags TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tasks (
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
  session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS schedules (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  config TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  next_run_at TEXT,
  last_run_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS runs (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  schedule_id TEXT REFERENCES schedules(id) ON DELETE SET NULL,
  agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL,
  state TEXT NOT NULL DEFAULT 'pending',
  trigger TEXT NOT NULL,
  exit_code INTEGER,
  pid INTEGER,
  log_path TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  duration_ms INTEGER,
  retry_count INTEGER NOT NULL DEFAULT 0,
  parent_run_id TEXT REFERENCES runs(id),
  metadata TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_runs_task_id ON runs(task_id);
CREATE INDEX IF NOT EXISTS idx_runs_state ON runs(state);
CREATE INDEX IF NOT EXISTS idx_runs_created_at ON runs(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_schedules_task_id ON schedules(task_id);
CREATE INDEX IF NOT EXISTS idx_schedules_next_run_at ON schedules(next_run_at) WHERE enabled = 1;
CREATE INDEX IF NOT EXISTS idx_tasks_agent_id ON tasks(agent_id);
CREATE INDEX IF NOT EXISTS idx_tasks_enabled ON tasks(enabled);

-- Seed the default agent
INSERT OR IGNORE INTO agents (id, name, description, state, max_concurrent_runs, created_at, updated_at)
VALUES (
  '01HZDEFAULTDEFAULTDEFAULTDA',
  'Default',
  'Default agent for running tasks',
  'idle',
  10,
  datetime('now'),
  datetime('now')
);
