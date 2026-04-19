alter table work_items
  add column if not exists column_id text;

create index if not exists idx_work_items_user_project_column
  on work_items(user_id, project_id, column_id, position);

create table if not exists project_board_columns (
  user_id uuid references auth.users not null,
  id text not null,
  project_id text not null,
  name text not null,
  role text,
  is_default boolean not null default false,
  position double precision not null default 0,
  created_at text not null,
  updated_at text not null,
  primary key (user_id, id)
);

alter table project_board_columns enable row level security;

do $$ begin
  create policy "users manage own project_board_columns" on project_board_columns for all
    using (auth.uid() = user_id) with check (auth.uid() = user_id);
exception when duplicate_object then null;
end $$;

create index if not exists idx_project_board_columns_user_project_position
  on project_board_columns(user_id, project_id, position);

create index if not exists idx_project_board_columns_user_project_role
  on project_board_columns(user_id, project_id, role, position);

create unique index if not exists idx_project_board_columns_user_project_default
  on project_board_columns(user_id, project_id)
  where is_default = true;
