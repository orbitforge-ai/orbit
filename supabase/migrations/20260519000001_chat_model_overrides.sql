alter table chat_sessions add column if not exists model_provider_override text;
alter table chat_sessions add column if not exists model_override text;
