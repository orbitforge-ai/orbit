create table if not exists project_boards (
  user_id uuid references auth.users not null,
  id text not null,
  project_id text not null,
  name text not null,
  prefix text not null,
  movement_guide text not null default '{"version":1,"summary":"","columnRules":[],"transitions":[],"agentInstructions":""}',
  position double precision not null default 0,
  is_default boolean not null default false,
  created_at text not null,
  updated_at text not null,
  primary key (user_id, id),
  unique (user_id, project_id, prefix)
);

alter table project_boards enable row level security;

do $$ begin
  create policy "users manage own project_boards" on project_boards for all
    using (auth.uid() = user_id) with check (auth.uid() = user_id);
exception when duplicate_object then null;
end $$;

alter table project_boards
  add column if not exists movement_guide text not null default '{"version":1,"summary":"","columnRules":[],"transitions":[],"agentInstructions":""}';

alter table project_board_columns
  add column if not exists board_id text;

drop index if exists idx_project_board_columns_user_project_role;
drop index if exists idx_project_board_columns_user_project_default;

alter table project_board_columns
  drop column if exists role;

create index if not exists idx_project_boards_user_project_position
  on project_boards(user_id, project_id, position);

create unique index if not exists idx_project_boards_user_project_default
  on project_boards(user_id, project_id)
  where is_default = true;

create index if not exists idx_project_board_columns_user_board_position
  on project_board_columns(user_id, board_id, position);

create unique index if not exists idx_project_board_columns_user_board_default
  on project_board_columns(user_id, project_id, board_id)
  where is_default = true;
