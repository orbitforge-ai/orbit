-- project_workflows table (declarative workflow definitions)
create table if not exists project_workflows (
  user_id uuid references auth.users not null,
  id text not null,
  project_id text not null,
  name text not null,
  description text,
  enabled boolean not null default false,
  graph text not null default '{"nodes":[],"edges":[],"schemaVersion":1}',
  trigger_kind text not null default 'manual',
  trigger_config text not null default '{}',
  version bigint not null default 1,
  created_at text not null,
  updated_at text not null,
  primary key (user_id, id)
);

alter table project_workflows enable row level security;

do $$ begin
  create policy "users manage own project_workflows" on project_workflows for all
    using (auth.uid() = user_id) with check (auth.uid() = user_id);
exception when duplicate_object then null;
end $$;

create index if not exists idx_project_workflows_user_project
  on project_workflows(user_id, project_id);
