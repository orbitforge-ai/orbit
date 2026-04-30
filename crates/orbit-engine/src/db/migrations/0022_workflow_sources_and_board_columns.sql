CREATE TABLE IF NOT EXISTS project_board_columns (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    status      TEXT NOT NULL,
    position    REAL NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_project_board_columns_project_status
    ON project_board_columns(project_id, status);
CREATE INDEX IF NOT EXISTS idx_project_board_columns_project_position
    ON project_board_columns(project_id, position);

INSERT OR IGNORE INTO project_board_columns (id, project_id, name, status, position, created_at, updated_at)
SELECT 'col_' || p.id || '_backlog', p.id, 'Backlog', 'backlog', 1024.0, p.created_at, p.updated_at
FROM projects p;

INSERT OR IGNORE INTO project_board_columns (id, project_id, name, status, position, created_at, updated_at)
SELECT 'col_' || p.id || '_todo', p.id, 'Todo', 'todo', 2048.0, p.created_at, p.updated_at
FROM projects p;

INSERT OR IGNORE INTO project_board_columns (id, project_id, name, status, position, created_at, updated_at)
SELECT 'col_' || p.id || '_in_progress', p.id, 'In Progress', 'in_progress', 3072.0, p.created_at, p.updated_at
FROM projects p;

INSERT OR IGNORE INTO project_board_columns (id, project_id, name, status, position, created_at, updated_at)
SELECT 'col_' || p.id || '_blocked', p.id, 'Blocked', 'blocked', 4096.0, p.created_at, p.updated_at
FROM projects p;

INSERT OR IGNORE INTO project_board_columns (id, project_id, name, status, position, created_at, updated_at)
SELECT 'col_' || p.id || '_review', p.id, 'Review', 'review', 5120.0, p.created_at, p.updated_at
FROM projects p;

INSERT OR IGNORE INTO project_board_columns (id, project_id, name, status, position, created_at, updated_at)
SELECT 'col_' || p.id || '_done', p.id, 'Done', 'done', 6144.0, p.created_at, p.updated_at
FROM projects p;

INSERT OR IGNORE INTO project_board_columns (id, project_id, name, status, position, created_at, updated_at)
SELECT 'col_' || p.id || '_cancelled', p.id, 'Cancelled', 'cancelled', 7168.0, p.created_at, p.updated_at
FROM projects p;

ALTER TABLE work_items ADD COLUMN column_id TEXT REFERENCES project_board_columns(id) ON DELETE SET NULL;

UPDATE work_items
SET column_id = (
    SELECT c.id
    FROM project_board_columns c
    WHERE c.project_id = work_items.project_id
      AND c.status = work_items.status
    ORDER BY c.position ASC
    LIMIT 1
)
WHERE column_id IS NULL;

CREATE INDEX IF NOT EXISTS idx_work_items_project_column_position
    ON work_items(project_id, column_id, position);

CREATE TABLE IF NOT EXISTS workflow_seen_items (
    id           TEXT PRIMARY KEY,
    workflow_id  TEXT NOT NULL REFERENCES project_workflows(id) ON DELETE CASCADE,
    node_id      TEXT NOT NULL,
    source_key   TEXT NOT NULL,
    fingerprint  TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    UNIQUE(workflow_id, node_id, source_key, fingerprint)
);

CREATE INDEX IF NOT EXISTS idx_workflow_seen_items_workflow_node
    ON workflow_seen_items(workflow_id, node_id);
