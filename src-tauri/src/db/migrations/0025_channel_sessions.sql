-- Map an inbound trigger channel (plugin + channel + optional thread) to a
-- long-lived chat_session for a given agent. Used by triggers/spawn.rs so
-- every Discord/Slack message in the same place keeps conversational
-- context and auto-compaction state.

CREATE TABLE IF NOT EXISTS channel_sessions (
    agent_id            TEXT NOT NULL,
    plugin_id           TEXT NOT NULL,
    provider_channel_id TEXT NOT NULL,
    provider_thread_id  TEXT NOT NULL DEFAULT '',
    session_id          TEXT NOT NULL,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    PRIMARY KEY (agent_id, plugin_id, provider_channel_id, provider_thread_id)
);

CREATE INDEX IF NOT EXISTS idx_channel_sessions_session
    ON channel_sessions(session_id);
