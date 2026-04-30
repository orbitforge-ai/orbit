-- Migrate default agent from ULID to slug ID.
-- SQLite doesn't support renaming primary keys, so we:
-- 1. Temporarily disable FK checks
-- 2. Update all references
-- 3. Re-enable FK checks

PRAGMA foreign_keys=OFF;

-- Update the default agent ID
UPDATE agents SET id = 'default' WHERE id = '01HZDEFAULTDEFAULTDEFAULTDA';

-- Update all foreign key references
UPDATE tasks SET agent_id = 'default' WHERE agent_id = '01HZDEFAULTDEFAULTDEFAULTDA';
UPDATE runs SET agent_id = 'default' WHERE agent_id = '01HZDEFAULTDEFAULTDEFAULTDA';
UPDATE chat_sessions SET agent_id = 'default' WHERE agent_id = '01HZDEFAULTDEFAULTDEFAULTDA';

PRAGMA foreign_keys=ON;
