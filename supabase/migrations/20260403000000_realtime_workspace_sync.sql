-- Realtime + workspace sync infrastructure.
--
-- 1. workspace_objects manifest table (Storage file metadata + tombstones)
-- 2. Storage bucket `orbit-workspaces` with per-user RLS
-- 3. Add all synced tables to the supabase_realtime publication

-- ---------------------------------------------------------------------------
-- workspace_objects
-- ---------------------------------------------------------------------------

create table if not exists workspace_objects (
    user_id      uuid references auth.users not null,
    scope_type   text not null check (scope_type in ('agent', 'project')),
    scope_id     text not null,
    path         text not null,
    storage_path text not null,
    sha256       text not null default '',
    size_bytes   bigint not null default 0,
    mime_type    text,
    version      bigint not null default 0,
    deleted_at   text,
    updated_at   text not null,
    primary key (user_id, scope_type, scope_id, path)
);

alter table workspace_objects enable row level security;

do $$ begin
  create policy "users manage own workspace_objects" on workspace_objects for all
    using (auth.uid() = user_id) with check (auth.uid() = user_id);
exception when duplicate_object then null;
end $$;

create index if not exists idx_workspace_objects_scope
    on workspace_objects (user_id, scope_type, scope_id);

-- ---------------------------------------------------------------------------
-- Storage bucket
-- ---------------------------------------------------------------------------

insert into storage.buckets (id, name, public, file_size_limit, allowed_mime_types)
values ('orbit-workspaces', 'orbit-workspaces', false, 52428800, null)
on conflict (id) do nothing;

-- RLS policies: users can only access objects under their own prefix
-- (users/{auth.uid()}/...)

do $$ begin
  create policy "users can insert own workspace objects"
    on storage.objects for insert
    with check (
      bucket_id = 'orbit-workspaces'
      and (storage.foldername(name))[1] = 'users'
      and (storage.foldername(name))[2] = auth.uid()::text
    );
exception when duplicate_object then null;
end $$;

do $$ begin
  create policy "users can select own workspace objects"
    on storage.objects for select
    using (
      bucket_id = 'orbit-workspaces'
      and (storage.foldername(name))[1] = 'users'
      and (storage.foldername(name))[2] = auth.uid()::text
    );
exception when duplicate_object then null;
end $$;

do $$ begin
  create policy "users can update own workspace objects"
    on storage.objects for update
    using (
      bucket_id = 'orbit-workspaces'
      and (storage.foldername(name))[1] = 'users'
      and (storage.foldername(name))[2] = auth.uid()::text
    );
exception when duplicate_object then null;
end $$;

do $$ begin
  create policy "users can delete own workspace objects"
    on storage.objects for delete
    using (
      bucket_id = 'orbit-workspaces'
      and (storage.foldername(name))[1] = 'users'
      and (storage.foldername(name))[2] = auth.uid()::text
    );
exception when duplicate_object then null;
end $$;

-- ---------------------------------------------------------------------------
-- Realtime publication
-- Add all tables that Orbit subscribes to via postgres_changes.
-- Run AFTER the tables exist (safe to re-run — alter publication ... add table
-- is idempotent if the table is already a member).
-- ---------------------------------------------------------------------------

do $$ begin
  alter publication supabase_realtime add table agents;
exception when others then null;
end $$;

do $$ begin
  alter publication supabase_realtime add table tasks;
exception when others then null;
end $$;

do $$ begin
  alter publication supabase_realtime add table schedules;
exception when others then null;
end $$;

do $$ begin
  alter publication supabase_realtime add table runs;
exception when others then null;
end $$;

do $$ begin
  alter publication supabase_realtime add table agent_conversations;
exception when others then null;
end $$;

do $$ begin
  alter publication supabase_realtime add table chat_sessions;
exception when others then null;
end $$;

do $$ begin
  alter publication supabase_realtime add table chat_messages;
exception when others then null;
end $$;

do $$ begin
  alter publication supabase_realtime add table chat_compaction_summaries;
exception when others then null;
end $$;

do $$ begin
  alter publication supabase_realtime add table bus_messages;
exception when others then null;
end $$;

do $$ begin
  alter publication supabase_realtime add table bus_subscriptions;
exception when others then null;
end $$;

do $$ begin
  alter publication supabase_realtime add table users;
exception when others then null;
end $$;

do $$ begin
  alter publication supabase_realtime add table memory_extraction_log;
exception when others then null;
end $$;

do $$ begin
  alter publication supabase_realtime add table projects;
exception when others then null;
end $$;

do $$ begin
  alter publication supabase_realtime add table project_agents;
exception when others then null;
end $$;

do $$ begin
  alter publication supabase_realtime add table workspace_objects;
exception when others then null;
end $$;
