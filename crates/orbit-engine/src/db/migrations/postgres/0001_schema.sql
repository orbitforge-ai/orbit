-- Phase C Postgres bootstrap schema.
--
-- This is a consolidated migration for new shared-runtime Postgres databases.
-- It represents the current SQLite schema after migrations 0001-0031, with
-- tenant_id present from table creation and RLS forced on every tenant table.

DO $$
BEGIN
    CREATE ROLE application_role NOLOGIN;
EXCEPTION
    WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION orbit_current_tenant_id()
RETURNS text
LANGUAGE sql
STABLE
AS $$
    SELECT NULLIF(current_setting('app.tenant_id', true), '')
$$;

CREATE TABLE IF NOT EXISTS agents (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    state TEXT NOT NULL DEFAULT 'idle',
    max_concurrent_runs BIGINT NOT NULL DEFAULT 5,
    heartbeat_at TEXT,
    model_config TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    is_default BOOLEAN NOT NULL DEFAULT false,
    created_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS project_agents (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    is_default BOOLEAN NOT NULL DEFAULT false,
    added_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    PRIMARY KEY (project_id, agent_id)
);

CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    kind TEXT NOT NULL,
    config TEXT NOT NULL,
    max_duration_seconds BIGINT NOT NULL DEFAULT 3600,
    max_retries BIGINT NOT NULL DEFAULT 0,
    retry_delay_seconds BIGINT NOT NULL DEFAULT 60,
    concurrency_policy TEXT NOT NULL DEFAULT 'allow',
    tags TEXT NOT NULL DEFAULT '[]',
    agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL,
    enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    tenant_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS project_workflows (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    enabled BOOLEAN NOT NULL DEFAULT false,
    graph TEXT NOT NULL DEFAULT '{"nodes":[],"edges":[],"schemaVersion":1}',
    trigger_kind TEXT NOT NULL DEFAULT 'manual',
    trigger_config TEXT NOT NULL DEFAULT '{}',
    version BIGINT NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS schedules (
    id TEXT PRIMARY KEY,
    task_id TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    workflow_id TEXT REFERENCES project_workflows(id) ON DELETE CASCADE,
    target_kind TEXT NOT NULL DEFAULT 'task',
    kind TEXT NOT NULL,
    config TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT true,
    next_run_at TEXT,
    last_run_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    CHECK (
        (target_kind = 'task' AND task_id IS NOT NULL AND workflow_id IS NULL)
     OR (target_kind = 'workflow' AND workflow_id IS NOT NULL AND task_id IS NULL)
    )
);

CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY,
    task_id TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    schedule_id TEXT REFERENCES schedules(id) ON DELETE SET NULL,
    agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL,
    state TEXT NOT NULL DEFAULT 'pending',
    trigger TEXT NOT NULL,
    exit_code BIGINT,
    pid BIGINT,
    log_path TEXT,
    started_at TEXT,
    finished_at TEXT,
    duration_ms BIGINT,
    retry_count BIGINT NOT NULL DEFAULT 0,
    parent_run_id TEXT REFERENCES runs(id),
    metadata TEXT NOT NULL DEFAULT '{}',
    chain_depth BIGINT NOT NULL DEFAULT 0,
    source_bus_message_id TEXT,
    is_sub_agent BOOLEAN NOT NULL DEFAULT false,
    created_at TEXT NOT NULL,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    tenant_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_conversations (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    run_id TEXT REFERENCES runs(id) ON DELETE CASCADE,
    messages TEXT NOT NULL DEFAULT '[]',
    total_input_tokens BIGINT NOT NULL DEFAULT 0,
    total_output_tokens BIGINT NOT NULL DEFAULT 0,
    iterations BIGINT NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS bus_messages (
    id TEXT PRIMARY KEY,
    from_agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    from_run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    from_session_id TEXT,
    to_agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    to_run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    to_session_id TEXT,
    kind TEXT NOT NULL DEFAULT 'direct',
    event_type TEXT,
    payload TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'delivered',
    created_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL
);

DO $$
BEGIN
    ALTER TABLE runs
        ADD CONSTRAINT runs_source_bus_message_fk
        FOREIGN KEY (source_bus_message_id) REFERENCES bus_messages(id) ON DELETE SET NULL;
EXCEPTION
    WHEN duplicate_object THEN NULL;
END $$;

CREATE TABLE IF NOT EXISTS bus_subscriptions (
    id TEXT PRIMARY KEY,
    subscriber_agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    source_agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    task_id TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    payload_template TEXT NOT NULL DEFAULT '{}',
    enabled BOOLEAN NOT NULL DEFAULT true,
    max_chain_depth BIGINT NOT NULL DEFAULT 10,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS chat_sessions (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    archived BOOLEAN NOT NULL DEFAULT false,
    last_input_tokens BIGINT,
    session_type TEXT NOT NULL DEFAULT 'user_chat',
    parent_session_id TEXT REFERENCES chat_sessions(id) ON DELETE SET NULL,
    source_bus_message_id TEXT REFERENCES bus_messages(id) ON DELETE SET NULL,
    chain_depth BIGINT NOT NULL DEFAULT 0,
    execution_state TEXT,
    finish_summary TEXT,
    terminal_error TEXT,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    allow_sub_agents BOOLEAN NOT NULL DEFAULT false,
    worktree_name TEXT,
    worktree_branch TEXT,
    worktree_path TEXT,
    compaction_failure_count BIGINT NOT NULL DEFAULT 0,
    compaction_last_failure_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL
);

DO $$
BEGIN
    ALTER TABLE bus_messages
        ADD CONSTRAINT bus_messages_from_session_fk
        FOREIGN KEY (from_session_id) REFERENCES chat_sessions(id) ON DELETE SET NULL;
EXCEPTION
    WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE bus_messages
        ADD CONSTRAINT bus_messages_to_session_fk
        FOREIGN KEY (to_session_id) REFERENCES chat_sessions(id) ON DELETE SET NULL;
EXCEPTION
    WHEN duplicate_object THEN NULL;
END $$;

CREATE TABLE IF NOT EXISTS chat_messages (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    token_count BIGINT,
    is_compacted BOOLEAN NOT NULL DEFAULT false,
    created_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS message_reactions (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL REFERENCES chat_messages(id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    emoji TEXT NOT NULL,
    created_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    UNIQUE(message_id, emoji)
);

CREATE TABLE IF NOT EXISTS chat_compaction_summaries (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    summary_message_id TEXT NOT NULL REFERENCES chat_messages(id) ON DELETE CASCADE,
    compacted_message_ids TEXT NOT NULL,
    original_token_count BIGINT,
    summary_token_count BIGINT NOT NULL,
    created_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS active_session_skills (
    session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    skill_name TEXT NOT NULL,
    instructions TEXT NOT NULL,
    source_path TEXT,
    activated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    PRIMARY KEY (session_id, skill_name)
);

CREATE TABLE IF NOT EXISTS discovered_session_skills (
    session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    skill_name TEXT NOT NULL,
    source_path TEXT,
    discovered_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    PRIMARY KEY (session_id, skill_name)
);

CREATE TABLE IF NOT EXISTS memory_extraction_log (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    memories_extracted BIGINT NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS project_boards (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    prefix TEXT NOT NULL,
    position DOUBLE PRECISION NOT NULL DEFAULT 0,
    is_default BOOLEAN NOT NULL DEFAULT false,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    UNIQUE (project_id, prefix)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_project_boards_project_default
    ON project_boards(project_id)
    WHERE is_default = true;

CREATE TABLE IF NOT EXISTS project_board_columns (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    board_id TEXT REFERENCES project_boards(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    role TEXT,
    is_default BOOLEAN NOT NULL DEFAULT false,
    position DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS work_items (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    board_id TEXT REFERENCES project_boards(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    description TEXT,
    kind TEXT NOT NULL DEFAULT 'task',
    column_id TEXT REFERENCES project_board_columns(id) ON DELETE SET NULL,
    status TEXT NOT NULL DEFAULT 'backlog',
    priority BIGINT NOT NULL DEFAULT 0,
    assignee_agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL,
    created_by_agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL,
    parent_work_item_id TEXT REFERENCES work_items(id) ON DELETE SET NULL,
    position DOUBLE PRECISION NOT NULL DEFAULT 0,
    labels TEXT NOT NULL DEFAULT '[]',
    metadata TEXT NOT NULL DEFAULT '{}',
    blocked_reason TEXT,
    started_at TEXT,
    completed_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS work_item_comments (
    id TEXT PRIMARY KEY,
    work_item_id TEXT NOT NULL REFERENCES work_items(id) ON DELETE CASCADE,
    author_kind TEXT NOT NULL,
    author_agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL,
    body TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS work_item_events (
    id TEXT PRIMARY KEY,
    work_item_id TEXT NOT NULL REFERENCES work_items(id) ON DELETE CASCADE,
    actor_kind TEXT NOT NULL CHECK (actor_kind IN ('user','agent','system')),
    actor_agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_runs (
    id TEXT PRIMARY KEY,
    workflow_id TEXT NOT NULL REFERENCES project_workflows(id) ON DELETE CASCADE,
    workflow_version BIGINT NOT NULL,
    graph_snapshot TEXT NOT NULL,
    trigger_kind TEXT NOT NULL,
    trigger_data TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'queued',
    error TEXT,
    started_at TEXT,
    completed_at TEXT,
    created_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_run_steps (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    node_id TEXT NOT NULL,
    node_type TEXT NOT NULL,
    status TEXT NOT NULL,
    input TEXT NOT NULL DEFAULT '{}',
    output TEXT,
    error TEXT,
    started_at TEXT,
    completed_at TEXT,
    sequence BIGINT NOT NULL,
    created_at TEXT,
    tenant_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_seen_items (
    id TEXT PRIMARY KEY,
    workflow_id TEXT NOT NULL REFERENCES project_workflows(id) ON DELETE CASCADE,
    node_id TEXT NOT NULL,
    source_key TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    created_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    UNIQUE(workflow_id, node_id, source_key, fingerprint)
);

CREATE TABLE IF NOT EXISTS plugin_entities (
    id TEXT PRIMARY KEY,
    plugin_id TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    data TEXT NOT NULL,
    created_by_agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS plugin_entity_relations (
    id TEXT PRIMARY KEY,
    from_kind TEXT NOT NULL,
    from_type TEXT NOT NULL,
    from_id TEXT NOT NULL,
    to_kind TEXT NOT NULL,
    to_type TEXT NOT NULL,
    to_id TEXT NOT NULL,
    relation TEXT NOT NULL,
    created_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    UNIQUE(from_id, to_id, relation)
);

CREATE TABLE IF NOT EXISTS plugin_workflow_subscriptions (
    id TEXT PRIMARY KEY,
    plugin_id TEXT NOT NULL,
    workflow_id TEXT NOT NULL REFERENCES project_workflows(id) ON DELETE CASCADE,
    trigger_kind TEXT NOT NULL,
    subscription_id TEXT NOT NULL,
    config TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT,
    enabled BOOLEAN NOT NULL DEFAULT true,
    tenant_id TEXT NOT NULL,
    UNIQUE(plugin_id, workflow_id, trigger_kind)
);

CREATE TABLE IF NOT EXISTS channel_sessions (
    agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    plugin_id TEXT NOT NULL,
    provider_channel_id TEXT NOT NULL,
    provider_thread_id TEXT NOT NULL DEFAULT '',
    session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    PRIMARY KEY (agent_id, plugin_id, provider_channel_id, provider_thread_id)
);

CREATE TABLE IF NOT EXISTS agent_tasks (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    subject TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    active_form TEXT,
    blocked_by TEXT NOT NULL DEFAULT '[]',
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_agents_tenant_created ON agents(tenant_id, created_at);
CREATE INDEX IF NOT EXISTS idx_projects_tenant_created ON projects(tenant_id, created_at);
CREATE INDEX IF NOT EXISTS idx_project_agents_agent ON project_agents(tenant_id, agent_id);
CREATE INDEX IF NOT EXISTS idx_tasks_tenant_created ON tasks(tenant_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tasks_agent_id ON tasks(tenant_id, agent_id);
CREATE INDEX IF NOT EXISTS idx_tasks_enabled ON tasks(tenant_id, enabled);
CREATE INDEX IF NOT EXISTS idx_schedules_task_id ON schedules(tenant_id, task_id);
CREATE INDEX IF NOT EXISTS idx_schedules_workflow_id ON schedules(tenant_id, workflow_id);
CREATE INDEX IF NOT EXISTS idx_schedules_next_run_at ON schedules(tenant_id, next_run_at) WHERE enabled = true;
CREATE INDEX IF NOT EXISTS idx_runs_task_id ON runs(tenant_id, task_id);
CREATE INDEX IF NOT EXISTS idx_runs_state ON runs(tenant_id, state);
CREATE INDEX IF NOT EXISTS idx_runs_created_at ON runs(tenant_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_runs_project_id ON runs(tenant_id, project_id);
CREATE INDEX IF NOT EXISTS idx_bus_messages_from_agent ON bus_messages(tenant_id, from_agent_id);
CREATE INDEX IF NOT EXISTS idx_bus_messages_to_agent ON bus_messages(tenant_id, to_agent_id);
CREATE INDEX IF NOT EXISTS idx_bus_messages_created_at ON bus_messages(tenant_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_bus_subs_source ON bus_subscriptions(tenant_id, source_agent_id, event_type);
CREATE INDEX IF NOT EXISTS idx_bus_subs_subscriber ON bus_subscriptions(tenant_id, subscriber_agent_id);
CREATE INDEX IF NOT EXISTS idx_chat_sessions_agent ON chat_sessions(tenant_id, agent_id);
CREATE INDEX IF NOT EXISTS idx_chat_sessions_project ON chat_sessions(tenant_id, project_id);
CREATE INDEX IF NOT EXISTS idx_chat_messages_session ON chat_messages(tenant_id, session_id, created_at ASC);
CREATE INDEX IF NOT EXISTS idx_message_reactions_session ON message_reactions(tenant_id, session_id);
CREATE INDEX IF NOT EXISTS idx_active_session_skills_session ON active_session_skills(tenant_id, session_id, activated_at);
CREATE INDEX IF NOT EXISTS idx_project_boards_project_position ON project_boards(tenant_id, project_id, position);
CREATE INDEX IF NOT EXISTS idx_project_board_columns_board_position ON project_board_columns(tenant_id, board_id, position);
CREATE INDEX IF NOT EXISTS idx_work_items_project_column_position ON work_items(tenant_id, project_id, column_id, position);
CREATE INDEX IF NOT EXISTS idx_work_items_board_position ON work_items(tenant_id, board_id, position);
CREATE INDEX IF NOT EXISTS idx_work_items_assignee_status ON work_items(tenant_id, assignee_agent_id, status);
CREATE INDEX IF NOT EXISTS idx_work_item_comments_work_item_created ON work_item_comments(tenant_id, work_item_id, created_at);
CREATE INDEX IF NOT EXISTS idx_work_item_events_item_created ON work_item_events(tenant_id, work_item_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_project_workflows_project ON project_workflows(tenant_id, project_id);
CREATE INDEX IF NOT EXISTS idx_workflow_runs_workflow ON workflow_runs(tenant_id, workflow_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_workflow_runs_status ON workflow_runs(tenant_id, status);
CREATE INDEX IF NOT EXISTS idx_workflow_run_steps_run ON workflow_run_steps(tenant_id, run_id, sequence);
CREATE INDEX IF NOT EXISTS idx_workflow_seen_items_workflow_node ON workflow_seen_items(tenant_id, workflow_id, node_id);
CREATE INDEX IF NOT EXISTS idx_plugin_entities_type ON plugin_entities(tenant_id, plugin_id, entity_type);
CREATE INDEX IF NOT EXISTS idx_plugin_entities_project ON plugin_entities(tenant_id, project_id);
CREATE INDEX IF NOT EXISTS idx_plugin_entity_relations_from ON plugin_entity_relations(tenant_id, from_kind, from_type, from_id);
CREATE INDEX IF NOT EXISTS idx_plugin_entity_relations_to ON plugin_entity_relations(tenant_id, to_kind, to_type, to_id);
CREATE INDEX IF NOT EXISTS idx_plugin_workflow_subs_workflow ON plugin_workflow_subscriptions(tenant_id, workflow_id);
CREATE INDEX IF NOT EXISTS idx_channel_sessions_session ON channel_sessions(tenant_id, session_id);
CREATE INDEX IF NOT EXISTS idx_agent_tasks_session ON agent_tasks(tenant_id, session_id, created_at DESC);

GRANT USAGE ON SCHEMA public TO application_role;

DO $$
DECLARE
    table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'active_session_skills',
        'agent_conversations',
        'agent_tasks',
        'agents',
        'bus_messages',
        'bus_subscriptions',
        'channel_sessions',
        'chat_compaction_summaries',
        'chat_messages',
        'chat_sessions',
        'discovered_session_skills',
        'memory_extraction_log',
        'message_reactions',
        'plugin_entities',
        'plugin_entity_relations',
        'plugin_workflow_subscriptions',
        'project_agents',
        'project_board_columns',
        'project_boards',
        'project_workflows',
        'projects',
        'runs',
        'schedules',
        'tasks',
        'users',
        'work_item_comments',
        'work_item_events',
        'work_items',
        'workflow_run_steps',
        'workflow_runs',
        'workflow_seen_items'
    ]
    LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', table_name);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', table_name);
        EXECUTE format('DROP POLICY IF EXISTS tenant_isolation ON %I', table_name);
        EXECUTE format(
            'CREATE POLICY tenant_isolation ON %I FOR ALL TO application_role USING (tenant_id = orbit_current_tenant_id()) WITH CHECK (tenant_id = orbit_current_tenant_id())',
            table_name
        );
        EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON %I TO application_role', table_name);
    END LOOP;
END $$;
