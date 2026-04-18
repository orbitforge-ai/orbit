-- work_items table (project board cards)
create table if not exists work_items (
  user_id uuid references auth.users not null,
  id text not null,
  project_id text not null,
  title text not null,
  description text,
  kind text not null default 'task',
  status text not null default 'backlog',
  priority integer not null default 0,
  assignee_agent_id text,
  created_by_agent_id text,
  parent_work_item_id text,
  position double precision not null default 0,
  labels text not null default '[]',
  metadata text not null default '{}',
  blocked_reason text,
  started_at text,
  completed_at text,
  created_at text not null,
  updated_at text not null,
  primary key (user_id, id)
);

alter table work_items enable row level security;

do $$ begin
  create policy "users manage own work_items" on work_items for all
    using (auth.uid() = user_id) with check (auth.uid() = user_id);
exception when duplicate_object then null;
end $$;

create index if not exists idx_work_items_user_project_status
  on work_items(user_id, project_id, status, position);
create index if not exists idx_work_items_user_assignee
  on work_items(user_id, assignee_agent_id, status);
create index if not exists idx_work_items_user_parent
  on work_items(user_id, parent_work_item_id);
