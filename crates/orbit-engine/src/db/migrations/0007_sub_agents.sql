-- Migration 7: Sub-agent support
-- Adds is_sub_agent flag and index on parent_run_id for efficient sub-agent queries.

ALTER TABLE runs ADD COLUMN is_sub_agent INTEGER NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_runs_parent_run_id ON runs(parent_run_id);
