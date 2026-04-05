-- message_reactions
create table if not exists message_reactions (
  user_id uuid references auth.users not null,
  id text not null,
  message_id text not null,
  session_id text not null,
  emoji text not null,
  created_at text not null,
  primary key (user_id, id),
  unique (user_id, message_id, emoji)
);

alter table message_reactions enable row level security;
create policy "users manage own message reactions" on message_reactions for all
  using (auth.uid() = user_id) with check (auth.uid() = user_id);
create index if not exists idx_message_reactions_session on message_reactions(user_id, session_id);
create index if not exists idx_message_reactions_message on message_reactions(user_id, message_id);
