-- Migration 31: Add tenant_id to every table.
--
-- Phase B / Phase C of the cloud-mode plan: every row gets a tenant_id so
-- the same engine binary can run in three modes —
--   * desktop / self-hosted single-tenant: rows default to 'local'
--   * paid SaaS tier (per-tenant Fly Machine + per-tenant SQLite):
--       rows default to the tenant's id, set on every connection
--   * free SaaS tier (shared Postgres with RLS): tenant_id is the RLS scope
--
-- Existing rows on existing local installs all become tenant_id = 'local'.
-- New writes inherit the connection-level default unless the engine is
-- explicitly running with a non-local tenant context.

ALTER TABLE active_session_skills        ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE agent_conversations          ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE agent_tasks                  ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE agents                       ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE bus_messages                 ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE bus_subscriptions            ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE channel_sessions             ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE chat_compaction_summaries    ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE chat_messages                ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE chat_sessions                ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE discovered_session_skills    ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE memory_extraction_log        ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE message_reactions            ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE plugin_entities              ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE plugin_entity_relations      ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE plugin_workflow_subscriptions ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE project_agents               ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE project_board_columns        ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE project_boards               ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE project_workflows            ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE projects                     ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE runs                         ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE schedules                    ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE tasks                        ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE users                        ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE work_item_comments           ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE work_item_events             ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE work_items                   ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE workflow_run_steps           ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE workflow_runs                ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE workflow_seen_items          ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'local';
