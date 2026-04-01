-- Users table for multi-user memory scoping.
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

-- Seed a default user.
INSERT OR IGNORE INTO users (id, name, is_default, created_at)
VALUES ('default_user', 'Default User', 1, datetime('now'));
