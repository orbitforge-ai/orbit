alter table chat_sessions add column if not exists workflow_run_id text;
alter table chat_sessions add column if not exists workflow_node_id text;

create index if not exists idx_chat_sessions_user_workflow_run_node
  on chat_sessions(user_id, workflow_run_id, workflow_node_id);
