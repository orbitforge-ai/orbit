-- Workspace file sync manifest.
-- Tracks which workspace files have been synced to Supabase Storage,
-- their content hash, version (Unix ms), and soft-delete tombstones.

CREATE TABLE IF NOT EXISTS workspace_objects (
    user_id      TEXT NOT NULL,
    scope_type   TEXT NOT NULL CHECK (scope_type IN ('agent', 'project')),
    scope_id     TEXT NOT NULL,
    path         TEXT NOT NULL,
    storage_path TEXT NOT NULL,
    sha256       TEXT NOT NULL DEFAULT '',
    size_bytes   INTEGER NOT NULL DEFAULT 0,
    mime_type    TEXT,
    version      INTEGER NOT NULL DEFAULT 0,
    deleted_at   TEXT,
    updated_at   TEXT NOT NULL,
    PRIMARY KEY (user_id, scope_type, scope_id, path)
);

CREATE INDEX IF NOT EXISTS idx_workspace_objects_scope
    ON workspace_objects (user_id, scope_type, scope_id);
