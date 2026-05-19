ALTER TABLE chat_sessions ADD COLUMN workflow_run_id TEXT DEFAULT NULL;
ALTER TABLE chat_sessions ADD COLUMN workflow_node_id TEXT DEFAULT NULL;

CREATE INDEX IF NOT EXISTS idx_chat_sessions_workflow_run_node
  ON chat_sessions(tenant_id, workflow_run_id, workflow_node_id);
