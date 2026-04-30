-- Migration 19: work item comments — append-only discussion thread per card.
--
-- A thin thread so humans and agents can leave context on a card during SDLC
-- handoffs. `author_kind` discriminates user vs agent; `author_agent_id` is
-- populated only when `author_kind = 'agent'`.

CREATE TABLE IF NOT EXISTS work_item_comments (
    id              TEXT PRIMARY KEY,
    work_item_id    TEXT NOT NULL REFERENCES work_items(id) ON DELETE CASCADE,
    author_kind     TEXT NOT NULL,
    author_agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL,
    body            TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_work_item_comments_work_item_created
    ON work_item_comments(work_item_id, created_at);
