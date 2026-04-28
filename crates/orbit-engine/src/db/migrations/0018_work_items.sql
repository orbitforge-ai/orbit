-- Migration 18: project work items (persistent kanban board cards).
--
-- Distinct from `tasks` (scheduled/executable automation jobs) and `agent_tasks`
-- (per-session scratch-pad TODOs). Work items are project-scoped units of
-- planned work with assignees, states, and relationships — manipulated by
-- humans or agents.

CREATE TABLE IF NOT EXISTS work_items (
    id                    TEXT PRIMARY KEY,
    project_id            TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title                 TEXT NOT NULL,
    description           TEXT,
    kind                  TEXT NOT NULL DEFAULT 'task',
    status                TEXT NOT NULL DEFAULT 'backlog',
    priority              INTEGER NOT NULL DEFAULT 0,
    assignee_agent_id     TEXT REFERENCES agents(id) ON DELETE SET NULL,
    created_by_agent_id   TEXT REFERENCES agents(id) ON DELETE SET NULL,
    parent_work_item_id   TEXT REFERENCES work_items(id) ON DELETE SET NULL,
    position              REAL NOT NULL DEFAULT 0,
    labels                TEXT NOT NULL DEFAULT '[]',
    metadata              TEXT NOT NULL DEFAULT '{}',
    blocked_reason        TEXT,
    started_at            TEXT,
    completed_at          TEXT,
    created_at            TEXT NOT NULL,
    updated_at            TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_work_items_project_status_position
    ON work_items(project_id, status, position);
CREATE INDEX IF NOT EXISTS idx_work_items_assignee_status
    ON work_items(assignee_agent_id, status);
CREATE INDEX IF NOT EXISTS idx_work_items_parent
    ON work_items(parent_work_item_id);
