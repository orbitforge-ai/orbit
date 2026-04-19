-- Project workflows: declarative graph definitions per project. Execution
-- happens in a future migration (workflow_runs); this table is inert
-- definitions only.
--
-- `graph` is the canonical JSON shape `{nodes, edges, schemaVersion}`.
-- `version` bumps on every save so future workflow_runs can snapshot the
-- definition that ran. `enabled` must be true for the future runtime to
-- fire scheduled triggers.
CREATE TABLE IF NOT EXISTS project_workflows (
    id              TEXT PRIMARY KEY,
    project_id      TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    description     TEXT,
    enabled         INTEGER NOT NULL DEFAULT 0,
    graph           TEXT NOT NULL DEFAULT '{"nodes":[],"edges":[],"schemaVersion":1}',
    trigger_kind    TEXT NOT NULL DEFAULT 'manual',
    trigger_config  TEXT NOT NULL DEFAULT '{}',
    version         INTEGER NOT NULL DEFAULT 1,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_project_workflows_project
    ON project_workflows(project_id);
