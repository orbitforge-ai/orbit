-- Projects: a shared workspace and organizational unit for grouping agents, tasks, and runs.

CREATE TABLE IF NOT EXISTS projects (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

-- Many-to-many join: agents assigned to projects.
-- is_default = 1 means this is the agent's primary project.
CREATE TABLE IF NOT EXISTS project_agents (
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    agent_id    TEXT NOT NULL REFERENCES agents(id)   ON DELETE CASCADE,
    is_default  INTEGER NOT NULL DEFAULT 0,
    added_at    TEXT NOT NULL,
    PRIMARY KEY (project_id, agent_id)
);
CREATE INDEX IF NOT EXISTS idx_project_agents_agent ON project_agents(agent_id);

-- Nullable project_id on tasks, sessions, and runs for project scoping.
ALTER TABLE tasks         ADD COLUMN project_id TEXT;
ALTER TABLE chat_sessions ADD COLUMN project_id TEXT;
ALTER TABLE runs          ADD COLUMN project_id TEXT;
