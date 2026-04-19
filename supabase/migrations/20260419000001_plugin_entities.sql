-- Plugin entities + relations mirror of local migration 0024. Schema is
-- plugin-agnostic: `data` is opaque JSONB, validated at the app layer.

create table if not exists plugin_entities (
  user_id uuid references auth.users not null,
  id text not null,
  plugin_id text not null,
  entity_type text not null,
  project_id text,
  data jsonb not null default '{}'::jsonb,
  created_by_agent_id text,
  created_at text not null,
  updated_at text not null,
  primary key (user_id, id)
);

alter table plugin_entities enable row level security;

do $$ begin
  create policy "users manage own plugin_entities" on plugin_entities for all
    using (auth.uid() = user_id) with check (auth.uid() = user_id);
exception when duplicate_object then null;
end $$;

create index if not exists idx_plugin_entities_type
  on plugin_entities(user_id, plugin_id, entity_type);
create index if not exists idx_plugin_entities_project
  on plugin_entities(user_id, project_id);

-- Polymorphic relation table.
create table if not exists plugin_entity_relations (
  user_id uuid references auth.users not null,
  id text not null,
  from_kind text not null,
  from_type text not null,
  from_id text not null,
  to_kind text not null,
  to_type text not null,
  to_id text not null,
  relation text not null,
  created_at text not null,
  primary key (user_id, id),
  unique (user_id, from_id, to_id, relation)
);

alter table plugin_entity_relations enable row level security;

do $$ begin
  create policy "users manage own plugin_entity_relations" on plugin_entity_relations for all
    using (auth.uid() = user_id) with check (auth.uid() = user_id);
exception when duplicate_object then null;
end $$;

create index if not exists idx_plugin_entity_relations_from
  on plugin_entity_relations(user_id, from_kind, from_type, from_id);
create index if not exists idx_plugin_entity_relations_to
  on plugin_entity_relations(user_id, to_kind, to_type, to_id);
