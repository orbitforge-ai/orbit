-- ============================================================
-- Orbit Supabase Schema
-- Run this in your Supabase SQL editor (Database → SQL Editor)
-- ============================================================

-- 1. Enable the Vault extension (Database → Extensions → supabase_vault)
--    Do this in the Supabase dashboard UI first, then run the rest here.

-- ============================================================
-- 2. API keys table (references vault.secrets for encrypted storage)
-- ============================================================
create table if not exists user_api_keys (
  user_id uuid references auth.users not null,
  provider text not null,   -- 'anthropic', 'brave', etc.
  secret_id uuid not null,  -- FK into vault.secrets (pgsodium encrypted)
  updated_at timestamptz default now(),
  primary key (user_id, provider)
);

alter table user_api_keys enable row level security;

create policy "users manage own api keys"
  on user_api_keys
  for all
  using (auth.uid() = user_id)
  with check (auth.uid() = user_id);

-- ============================================================
-- 3. Vault helper functions
-- ============================================================

-- List which providers have a stored key for the current user
create or replace function list_api_key_providers()
returns table(provider text)
language plpgsql
security definer
set search_path = public
as $$
begin
  return query
    select k.provider
    from user_api_keys k
    where k.user_id = auth.uid();
end;
$$;

-- Upsert an API key into Vault and record it in user_api_keys
create or replace function upsert_api_key(p_provider text, p_key text)
returns void
language plpgsql
security definer
set search_path = public
as $$
declare
  v_secret_id uuid;
  v_secret_name text;
begin
  v_secret_name := auth.uid()::text || '_' || p_provider;

  -- Create or update the secret in Vault
  select id into v_secret_id
  from vault.secrets
  where name = v_secret_name;

  if v_secret_id is null then
    select vault.create_secret(p_key, v_secret_name) into v_secret_id;
  else
    perform vault.update_secret(v_secret_id, p_key);
  end if;

  insert into user_api_keys (user_id, provider, secret_id, updated_at)
    values (auth.uid(), p_provider, v_secret_id, now())
    on conflict (user_id, provider) do update
      set secret_id = v_secret_id, updated_at = now();
end;
$$;

-- Retrieve a decrypted API key for the current user
create or replace function get_api_key(p_provider text)
returns text
language plpgsql
security definer
set search_path = public
as $$
begin
  return (
    select ds.decrypted_secret
    from user_api_keys k
    join vault.decrypted_secrets ds on ds.id = k.secret_id
    where k.user_id = auth.uid() and k.provider = p_provider
  );
end;
$$;

-- ============================================================
-- 4. Environment variables to set before building the app
-- ============================================================
-- In your shell or CI environment, before running `pnpm tauri build`:
--
--   export SUPABASE_URL="https://yourproject.supabase.co"
--   export SUPABASE_ANON_KEY="eyJ..."
--
-- Both values are in: Supabase dashboard → Settings → API
-- The anon key is safe to embed in the desktop app binary.
