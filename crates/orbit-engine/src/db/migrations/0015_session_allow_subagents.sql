-- Migration 15: persist whether a session may spawn nested sub-agents

ALTER TABLE chat_sessions ADD COLUMN allow_sub_agents INTEGER NOT NULL DEFAULT 0;
