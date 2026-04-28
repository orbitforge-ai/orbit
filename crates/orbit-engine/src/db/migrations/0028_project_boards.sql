-- Migration 28: multiple boards per project.
--
-- Introduces `project_boards` so a project can carry many independent boards.
-- Each board has a short uppercase alpha `prefix` (CORE, PLUGIN, ...) used to
-- render work items as PREFIX-XXXXXX (last 6 chars of the ULID, uppercased).
--
-- Existing columns and work items keep their implicit "one board per project"
-- relationship for now; the Rust migration runner materializes a default
-- board per project and backfills `board_id` on dependent rows immediately
-- after this SQL runs, so the UI sees no disruption.

CREATE TABLE IF NOT EXISTS project_boards (
    id         TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    prefix     TEXT NOT NULL,
    position   REAL NOT NULL DEFAULT 0,
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (project_id, prefix)
);

CREATE INDEX IF NOT EXISTS idx_project_boards_project_position
    ON project_boards(project_id, position);

CREATE UNIQUE INDEX IF NOT EXISTS idx_project_boards_project_default
    ON project_boards(project_id)
    WHERE is_default = 1;

ALTER TABLE project_board_columns ADD COLUMN board_id TEXT REFERENCES project_boards(id) ON DELETE CASCADE;

CREATE INDEX IF NOT EXISTS idx_project_board_columns_board_position
    ON project_board_columns(board_id, position);

ALTER TABLE work_items ADD COLUMN board_id TEXT REFERENCES project_boards(id) ON DELETE CASCADE;

CREATE INDEX IF NOT EXISTS idx_work_items_board_position
    ON work_items(board_id, position);
