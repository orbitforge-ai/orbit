CREATE TABLE IF NOT EXISTS active_session_skills (
    session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    skill_name TEXT NOT NULL,
    instructions TEXT NOT NULL,
    source_path TEXT DEFAULT NULL,
    activated_at TEXT NOT NULL,
    PRIMARY KEY (session_id, skill_name)
);

CREATE INDEX IF NOT EXISTS idx_active_session_skills_session
    ON active_session_skills(session_id, activated_at);

CREATE TABLE IF NOT EXISTS discovered_session_skills (
    session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    skill_name TEXT NOT NULL,
    discovered_at TEXT NOT NULL,
    PRIMARY KEY (session_id, skill_name)
);

CREATE INDEX IF NOT EXISTS idx_discovered_session_skills_session
    ON discovered_session_skills(session_id, discovered_at);
