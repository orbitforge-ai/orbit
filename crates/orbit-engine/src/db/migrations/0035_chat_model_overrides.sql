ALTER TABLE chat_sessions ADD COLUMN model_provider_override TEXT DEFAULT NULL;
ALTER TABLE chat_sessions ADD COLUMN model_override TEXT DEFAULT NULL;
