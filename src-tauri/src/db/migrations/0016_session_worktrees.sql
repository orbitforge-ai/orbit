-- Migration 16: persist active worktree state per session

ALTER TABLE chat_sessions ADD COLUMN worktree_name TEXT DEFAULT NULL;
ALTER TABLE chat_sessions ADD COLUMN worktree_branch TEXT DEFAULT NULL;
ALTER TABLE chat_sessions ADD COLUMN worktree_path TEXT DEFAULT NULL;
