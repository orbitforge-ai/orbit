-- Migration 29: scope the "one default column" uniqueness to (project_id, board_id).
--
-- Migration 23 created `idx_project_board_columns_project_default` as a unique
-- index over `project_id` where `is_default = 1`. That was correct when a
-- project had exactly one board. After migration 28 (multiple boards per
-- project) every board carries its own default column, so the old index
-- rejects the first column added to any non-default board with
-- "UNIQUE constraint failed: project_board_columns.project_id".

DROP INDEX IF EXISTS idx_project_board_columns_project_default;

CREATE UNIQUE INDEX IF NOT EXISTS idx_project_board_columns_board_default
    ON project_board_columns(project_id, board_id)
    WHERE is_default = 1;
