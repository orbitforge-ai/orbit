ALTER TABLE project_boards ADD COLUMN movement_guide TEXT NOT NULL DEFAULT '{"version":1,"summary":"","columnRules":[],"transitions":[],"agentInstructions":""}';

DROP INDEX IF EXISTS idx_project_board_columns_project_role;
DROP INDEX IF EXISTS idx_project_board_columns_project_status;

ALTER TABLE project_board_columns DROP COLUMN role;
ALTER TABLE project_board_columns DROP COLUMN status;
