alter table chat_sessions add column if not exists last_prompt_input_tokens bigint;
alter table chat_sessions add column if not exists last_turn_input_tokens bigint;

update chat_sessions
   set last_prompt_input_tokens = coalesce(last_prompt_input_tokens, last_input_tokens),
       last_turn_input_tokens = coalesce(last_turn_input_tokens, last_input_tokens)
 where last_input_tokens is not null
   and (last_prompt_input_tokens is null or last_turn_input_tokens is null);
