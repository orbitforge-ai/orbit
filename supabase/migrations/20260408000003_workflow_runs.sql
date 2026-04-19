-- workflow_runs (synced) and the schedules.target_kind / workflow_id columns
-- to let scheduled triggers fire workflow runs alongside task runs.

-- ─── workflow_runs ──────────────────────────────────────────────────────────
create table if not exists workflow_runs (
  user_id uuid references auth.users not null,
  id text not null,
  workflow_id text not null,
  workflow_version bigint not null,
  graph_snapshot jsonb not null default '{}'::jsonb,
  trigger_kind text not null,
  trigger_data jsonb not null default '{}'::jsonb,
  status text not null default 'queued',
  error text,
  started_at text,
  completed_at text,
  created_at text not null,
  primary key (user_id, id)
);

alter table workflow_runs enable row level security;

do $$ begin
  create policy "users manage own workflow_runs" on workflow_runs for all
    using (auth.uid() = user_id) with check (auth.uid() = user_id);
exception when duplicate_object then null;
end $$;

create index if not exists idx_workflow_runs_user_workflow
  on workflow_runs(user_id, workflow_id, created_at desc);
create index if not exists idx_workflow_runs_user_status
  on workflow_runs(user_id, status);

-- ─── schedules: add workflow_id + target_kind ───────────────────────────────
-- Existing rows keep target_kind = 'task' and workflow_id = NULL.
alter table schedules
  add column if not exists workflow_id text;

alter table schedules
  add column if not exists target_kind text not null default 'task';

create index if not exists idx_schedules_user_workflow
  on schedules(user_id, workflow_id);
