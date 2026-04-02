-- ============================================================
-- Orbit Supabase Schema
-- Run this in your Supabase SQL editor (Database → SQL Editor)
-- ============================================================

-- 1. Enable the Vault extension (Database → Extensions → supabase_vault)
--    Do this in the Supabase dashboard UI first, then run the rest here.

-- ============================================================
-- 2. API keys table (references vault.secrets for encrypted storage)
-- ============================================================
create table if not exists user_api_keys (
  user_id uuid references auth.users not null,
  provider text not null,
  secret_id uuid not null,
  updated_at timestamptz default now(),
  primary key (user_id, provider)
);

alter table user_api_keys enable row level security;

create policy "users manage own api keys"
  on user_api_keys for all
  using (auth.uid() = user_id)
  with check (auth.uid() = user_id);

-- ============================================================
-- 3. Vault helper functions
-- ============================================================

create or replace function list_api_key_providers()
returns table(provider text)
language plpgsql security definer set search_path = public as $$
begin
  return query select k.provider from user_api_keys k where k.user_id = auth.uid();
end; $$;

create or replace function upsert_api_key(p_provider text, p_key text)
returns void language plpgsql security definer set search_path = public as $$
declare v_secret_id uuid; v_secret_name text;
begin
  v_secret_name := auth.uid()::text || '_' || p_provider;
  select id into v_secret_id from vault.secrets where name = v_secret_name;
  if v_secret_id is null then
    select vault.create_secret(p_key, v_secret_name) into v_secret_id;
  else
    perform vault.update_secret(v_secret_id, p_key);
  end if;
  insert into user_api_keys (user_id, provider, secret_id, updated_at)
    values (auth.uid(), p_provider, v_secret_id, now())
    on conflict (user_id, provider) do update set secret_id = v_secret_id, updated_at = now();
end; $$;

create or replace function get_api_key(p_provider text)
returns text language plpgsql security definer set search_path = public as $$
begin
  return (
    select ds.decrypted_secret from user_api_keys k
    join vault.decrypted_secrets ds on ds.id = k.secret_id
    where k.user_id = auth.uid() and k.provider = p_provider
  );
end; $$;

-- ============================================================
-- 4. Core data tables
-- All tables use composite PK (user_id, id) for multi-user isolation.
-- RLS policies ensure users can only access their own rows.
-- Cross-table referential integrity is enforced by the application.
-- ============================================================

-- agents
create table if not exists agents (
  user_id uuid references auth.users not null,
  id text not null,
  name text not null,
  description text,
  state text not null default 'idle',
  max_concurrent_runs bigint not null default 5,
  heartbeat_at text,
  model_config text not null default '{}',
  created_at text not null,
  updated_at text not null,
  primary key (user_id, id)
);

alter table agents enable row level security;
create policy "users manage own agents" on agents for all
  using (auth.uid() = user_id) with check (auth.uid() = user_id);
create index if not exists idx_agents_user_id on agents(user_id);

-- tasks
create table if not exists tasks (
  user_id uuid references auth.users not null,
  id text not null,
  name text not null,
  description text,
  kind text not null,
  config jsonb not null default '{}',
  max_duration_seconds bigint not null default 3600,
  max_retries bigint not null default 0,
  retry_delay_seconds bigint not null default 60,
  concurrency_policy text not null default 'allow',
  tags jsonb not null default '[]',
  agent_id text,
  enabled boolean not null default true,
  created_at text not null,
  updated_at text not null,
  primary key (user_id, id)
);

alter table tasks enable row level security;
create policy "users manage own tasks" on tasks for all
  using (auth.uid() = user_id) with check (auth.uid() = user_id);
create index if not exists idx_tasks_user_id on tasks(user_id);
create index if not exists idx_tasks_agent_id on tasks(user_id, agent_id);

-- schedules
create table if not exists schedules (
  user_id uuid references auth.users not null,
  id text not null,
  task_id text not null,
  kind text not null,
  config jsonb not null default '{}',
  enabled boolean not null default true,
  next_run_at text,
  last_run_at text,
  created_at text not null,
  updated_at text not null,
  primary key (user_id, id)
);

alter table schedules enable row level security;
create policy "users manage own schedules" on schedules for all
  using (auth.uid() = user_id) with check (auth.uid() = user_id);
create index if not exists idx_schedules_user_id on schedules(user_id);
create index if not exists idx_schedules_task_id on schedules(user_id, task_id);

-- runs
create table if not exists runs (
  user_id uuid references auth.users not null,
  id text not null,
  task_id text not null,
  schedule_id text,
  agent_id text,
  state text not null default 'pending',
  trigger text not null,
  exit_code bigint,
  pid bigint,
  log_path text not null default '',
  started_at text,
  finished_at text,
  duration_ms bigint,
  retry_count bigint not null default 0,
  parent_run_id text,
  metadata jsonb not null default '{}',
  chain_depth bigint not null default 0,
  source_bus_message_id text,
  is_sub_agent boolean not null default false,
  created_at text not null,
  primary key (user_id, id)
);

alter table runs enable row level security;
create policy "users manage own runs" on runs for all
  using (auth.uid() = user_id) with check (auth.uid() = user_id);
create index if not exists idx_runs_user_id on runs(user_id);
create index if not exists idx_runs_state on runs(user_id, state);
create index if not exists idx_runs_created_at on runs(user_id, created_at desc);
create index if not exists idx_runs_task_id on runs(user_id, task_id);

-- agent_conversations
create table if not exists agent_conversations (
  user_id uuid references auth.users not null,
  id text not null,
  agent_id text not null,
  run_id text not null,
  messages jsonb not null default '[]',
  total_input_tokens bigint not null default 0,
  total_output_tokens bigint not null default 0,
  iterations bigint not null default 0,
  created_at text not null,
  updated_at text not null,
  primary key (user_id, id)
);

alter table agent_conversations enable row level security;
create policy "users manage own conversations" on agent_conversations for all
  using (auth.uid() = user_id) with check (auth.uid() = user_id);
create index if not exists idx_conversations_run_id on agent_conversations(user_id, run_id);

-- chat_sessions
create table if not exists chat_sessions (
  user_id uuid references auth.users not null,
  id text not null,
  agent_id text not null,
  title text not null default 'New Chat',
  archived boolean not null default false,
  last_input_tokens bigint,
  session_type text not null default 'user_chat',
  parent_session_id text,
  source_bus_message_id text,
  chain_depth bigint not null default 0,
  execution_state text,
  finish_summary text,
  terminal_error text,
  created_at text not null,
  updated_at text not null,
  primary key (user_id, id)
);

alter table chat_sessions enable row level security;
create policy "users manage own chat sessions" on chat_sessions for all
  using (auth.uid() = user_id) with check (auth.uid() = user_id);
create index if not exists idx_chat_sessions_user_id on chat_sessions(user_id);
create index if not exists idx_chat_sessions_agent_id on chat_sessions(user_id, agent_id);
create index if not exists idx_chat_sessions_updated_at on chat_sessions(user_id, updated_at desc);

-- chat_messages
create table if not exists chat_messages (
  user_id uuid references auth.users not null,
  id text not null,
  session_id text not null,
  role text not null,
  content text not null,
  token_count bigint,
  is_compacted boolean not null default false,
  created_at text not null,
  primary key (user_id, id)
);

alter table chat_messages enable row level security;
create policy "users manage own chat messages" on chat_messages for all
  using (auth.uid() = user_id) with check (auth.uid() = user_id);
create index if not exists idx_chat_messages_session on chat_messages(user_id, session_id, created_at asc);

-- chat_compaction_summaries
create table if not exists chat_compaction_summaries (
  user_id uuid references auth.users not null,
  id text not null,
  session_id text not null,
  summary_message_id text not null,
  compacted_message_ids jsonb not null default '[]',
  original_token_count bigint not null default 0,
  summary_token_count bigint not null default 0,
  created_at text not null,
  primary key (user_id, id)
);

alter table chat_compaction_summaries enable row level security;
create policy "users manage own compaction summaries" on chat_compaction_summaries for all
  using (auth.uid() = user_id) with check (auth.uid() = user_id);
create index if not exists idx_compaction_session on chat_compaction_summaries(user_id, session_id);

-- bus_messages
create table if not exists bus_messages (
  user_id uuid references auth.users not null,
  id text not null,
  from_agent_id text not null,
  from_run_id text,
  from_session_id text,
  to_agent_id text not null,
  to_run_id text,
  to_session_id text,
  kind text not null default 'direct',
  event_type text,
  payload jsonb not null default '{}',
  status text not null default 'delivered',
  created_at text not null,
  primary key (user_id, id)
);

alter table bus_messages enable row level security;
create policy "users manage own bus messages" on bus_messages for all
  using (auth.uid() = user_id) with check (auth.uid() = user_id);
create index if not exists idx_bus_messages_user_id on bus_messages(user_id);
create index if not exists idx_bus_messages_created_at on bus_messages(user_id, created_at desc);

-- bus_subscriptions
create table if not exists bus_subscriptions (
  user_id uuid references auth.users not null,
  id text not null,
  subscriber_agent_id text not null,
  source_agent_id text not null,
  event_type text not null,
  task_id text not null,
  payload_template text not null default '{}',
  enabled boolean not null default true,
  max_chain_depth bigint not null default 10,
  created_at text not null,
  updated_at text not null,
  primary key (user_id, id)
);

alter table bus_subscriptions enable row level security;
create policy "users manage own bus subscriptions" on bus_subscriptions for all
  using (auth.uid() = user_id) with check (auth.uid() = user_id);
create index if not exists idx_bus_subs_user_id on bus_subscriptions(user_id);

-- users (app-level user profiles, distinct from auth.users)
create table if not exists users (
  user_id uuid references auth.users not null,
  id text not null,
  name text not null,
  is_default boolean not null default false,
  created_at text not null,
  primary key (user_id, id)
);

alter table users enable row level security;
create policy "users manage own profiles" on users for all
  using (auth.uid() = user_id) with check (auth.uid() = user_id);

-- memory_extraction_log
create table if not exists memory_extraction_log (
  user_id uuid references auth.users not null,
  id text not null,
  session_id text,
  agent_id text,
  memories_extracted bigint not null default 0,
  status text not null default 'pending',
  created_at text not null,
  primary key (user_id, id)
);

alter table memory_extraction_log enable row level security;
create policy "users manage own memory log" on memory_extraction_log for all
  using (auth.uid() = user_id) with check (auth.uid() = user_id);

-- ============================================================
-- 5. Environment variables to set before building the app
-- ============================================================
-- Create src-tauri/.env with:
--   SUPABASE_URL=https://yourproject.supabase.co
--   SUPABASE_ANON_KEY=eyJ...
--
-- Both values are in: Supabase dashboard → Settings → API
-- The anon key is safe to embed in the desktop app binary (RLS enforces isolation).
