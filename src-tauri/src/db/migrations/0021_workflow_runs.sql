-- ─── workflow_runs ───────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS workflow_runs (
    id                TEXT PRIMARY KEY,
    workflow_id       TEXT NOT NULL REFERENCES project_workflows(id) ON DELETE CASCADE,
    workflow_version  INTEGER NOT NULL,
    graph_snapshot    TEXT NOT NULL,
    trigger_kind      TEXT NOT NULL,
    trigger_data      TEXT NOT NULL DEFAULT '{}',
    status            TEXT NOT NULL DEFAULT 'queued',
    error             TEXT,
    started_at        TEXT,
    completed_at      TEXT,
    created_at        TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_workflow_runs_workflow
  ON workflow_runs(workflow_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_workflow_runs_status
  ON workflow_runs(status);

-- ─── workflow_run_steps ──────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS workflow_run_steps (
    id            TEXT PRIMARY KEY,
    run_id        TEXT NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    node_id       TEXT NOT NULL,
    node_type     TEXT NOT NULL,
    status        TEXT NOT NULL,
    input         TEXT NOT NULL DEFAULT '{}',
    output        TEXT,
    error         TEXT,
    started_at    TEXT,
    completed_at  TEXT,
    sequence      INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_workflow_run_steps_run
  ON workflow_run_steps(run_id, sequence);

-- ─── schedules: rebuild to add workflow_id + target_kind ─────────────────────
-- The existing `kind` column means 'recurring' | 'one_shot' | 'triggered'.
-- Adding the task/workflow discriminator as `target_kind` avoids the clash.
-- SQLite cannot ALTER an existing NOT NULL column, so the table is rebuilt.

DROP INDEX IF EXISTS idx_schedules_task_id;
DROP INDEX IF EXISTS idx_schedules_next_run_at;

CREATE TABLE schedules_new (
    id            TEXT PRIMARY KEY,
    task_id       TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    workflow_id   TEXT REFERENCES project_workflows(id) ON DELETE CASCADE,
    target_kind   TEXT NOT NULL DEFAULT 'task',
    kind          TEXT NOT NULL,
    config        TEXT NOT NULL,
    enabled       INTEGER NOT NULL DEFAULT 1,
    next_run_at   TEXT,
    last_run_at   TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    CHECK (
        (target_kind = 'task' AND task_id IS NOT NULL AND workflow_id IS NULL)
     OR (target_kind = 'workflow' AND workflow_id IS NOT NULL AND task_id IS NULL)
    )
);

INSERT INTO schedules_new
    (id, task_id, workflow_id, target_kind, kind, config, enabled,
     next_run_at, last_run_at, created_at, updated_at)
SELECT
    id, task_id, NULL, 'task', kind, config, enabled,
    next_run_at, last_run_at, created_at, updated_at
FROM schedules;

DROP TABLE schedules;
ALTER TABLE schedules_new RENAME TO schedules;

CREATE INDEX IF NOT EXISTS idx_schedules_task_id ON schedules(task_id);
CREATE INDEX IF NOT EXISTS idx_schedules_workflow_id ON schedules(workflow_id);
CREATE INDEX IF NOT EXISTS idx_schedules_next_run_at
  ON schedules(next_run_at) WHERE enabled = 1;
