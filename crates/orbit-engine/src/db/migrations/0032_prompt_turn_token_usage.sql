-- Migration 32: Split current prompt usage from cumulative turn usage.
--
-- `last_input_tokens` originally stored cumulative input tokens across every
-- LLM call in the latest turn. The context gauge needs the most recent single
-- prompt size instead, while cumulative input remains useful for diagnostics.

ALTER TABLE chat_sessions ADD COLUMN last_prompt_input_tokens INTEGER DEFAULT NULL;
ALTER TABLE chat_sessions ADD COLUMN last_turn_input_tokens INTEGER DEFAULT NULL;

UPDATE chat_sessions
   SET last_prompt_input_tokens = last_input_tokens,
       last_turn_input_tokens = last_input_tokens
 WHERE last_input_tokens IS NOT NULL;
