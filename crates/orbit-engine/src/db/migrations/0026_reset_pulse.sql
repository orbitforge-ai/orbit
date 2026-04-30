-- Pulse is moving from agent-global to per-(agent, project). Old records are
-- keyed only by agent_id with no project scope, so wipe them — users will
-- reconfigure pulses per project from the new Project → Scheduled tab.

DELETE FROM schedules WHERE task_id IN (SELECT id FROM tasks WHERE tags LIKE '%"pulse"%');
DELETE FROM chat_sessions WHERE session_type = 'pulse';
DELETE FROM tasks WHERE tags LIKE '%"pulse"%';
