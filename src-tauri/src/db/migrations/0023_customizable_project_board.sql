ALTER TABLE project_board_columns ADD COLUMN role TEXT;
ALTER TABLE project_board_columns ADD COLUMN is_default INTEGER NOT NULL DEFAULT 0;

UPDATE project_board_columns
SET role = COALESCE(role, status)
WHERE role IS NULL;

UPDATE project_board_columns
SET is_default = 1
WHERE id IN (
    SELECT id
    FROM project_board_columns c
    WHERE c.role = 'backlog'
    ORDER BY c.project_id ASC, c.position ASC, c.created_at ASC
);

UPDATE project_board_columns
SET is_default = 1
WHERE project_id IN (
    SELECT project_id
    FROM project_board_columns
    GROUP BY project_id
    HAVING SUM(CASE WHEN is_default = 1 THEN 1 ELSE 0 END) = 0
)
AND id IN (
    SELECT c.id
    FROM project_board_columns c
    WHERE c.project_id = project_board_columns.project_id
    ORDER BY c.position ASC, c.created_at ASC
    LIMIT 1
);

DROP INDEX IF EXISTS idx_project_board_columns_project_status;

CREATE INDEX IF NOT EXISTS idx_project_board_columns_project_role
    ON project_board_columns(project_id, role, position);

CREATE UNIQUE INDEX IF NOT EXISTS idx_project_board_columns_project_default
    ON project_board_columns(project_id)
    WHERE is_default = 1;
