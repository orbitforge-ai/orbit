-- Migration 30: work_item_events — per-card timeline of field and status changes.
--
-- Feeds the Activity tab in the new work-item modal. Written by the
-- work_items commands (create/update/move/claim/block/complete/comments).
-- Cascades on card delete; no separate audit log of deletes for now.

CREATE TABLE work_item_events (
    id TEXT PRIMARY KEY NOT NULL,
    work_item_id TEXT NOT NULL REFERENCES work_items(id) ON DELETE CASCADE,
    actor_kind TEXT NOT NULL CHECK (actor_kind IN ('user','agent','system')),
    actor_agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX idx_work_item_events_item_created
    ON work_item_events(work_item_id, created_at DESC);
