-- work_item_comments table (discussion thread per card)
create table if not exists work_item_comments (
  user_id uuid references auth.users not null,
  id text not null,
  work_item_id text not null,
  author_kind text not null,
  author_agent_id text,
  body text not null,
  created_at text not null,
  updated_at text not null,
  primary key (user_id, id)
);

alter table work_item_comments enable row level security;

do $$ begin
  create policy "users manage own work_item_comments" on work_item_comments for all
    using (auth.uid() = user_id) with check (auth.uid() = user_id);
exception when duplicate_object then null;
end $$;

create index if not exists idx_work_item_comments_user_work_item
  on work_item_comments(user_id, work_item_id, created_at);
