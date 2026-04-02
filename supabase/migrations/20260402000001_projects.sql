-- projects table
create table if not exists projects (
  user_id uuid references auth.users not null,
  id text not null,
  name text not null,
  description text,
  created_at text not null,
  updated_at text not null,
  primary key (user_id, id)
);

alter table projects enable row level security;

do $$ begin
  create policy "users manage own projects" on projects for all
    using (auth.uid() = user_id) with check (auth.uid() = user_id);
exception when duplicate_object then null;
end $$;

create index if not exists idx_projects_user_id on projects(user_id);

-- project_agents join table
create table if not exists project_agents (
  user_id uuid references auth.users not null,
  project_id text not null,
  agent_id text not null,
  is_default boolean not null default false,
  added_at text not null,
  primary key (user_id, project_id, agent_id)
);

alter table project_agents enable row level security;

do $$ begin
  create policy "users manage own project_agents" on project_agents for all
    using (auth.uid() = user_id) with check (auth.uid() = user_id);
exception when duplicate_object then null;
end $$;

create index if not exists idx_project_agents_user on project_agents(user_id);
create index if not exists idx_project_agents_agent on project_agents(user_id, agent_id);

-- add project_id to existing tables
alter table tasks add column if not exists project_id text;
alter table runs add column if not exists project_id text;
alter table chat_sessions add column if not exists project_id text;

create index if not exists idx_tasks_project_id on tasks(user_id, project_id);
create index if not exists idx_runs_project_id on runs(user_id, project_id);
create index if not exists idx_chat_sessions_project_id on chat_sessions(user_id, project_id);
