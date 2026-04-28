-- Plugin system V1: storage for plugin-contributed entities and workflow
-- subscriptions. Plugins cannot ship their own SQL, so all plugin data lives
-- in these generic tables with `data` as opaque JSON validated at the app
-- layer against manifest JSON Schemas.

CREATE TABLE IF NOT EXISTS plugin_entities (
    id                  TEXT PRIMARY KEY,
    plugin_id           TEXT NOT NULL,
    entity_type         TEXT NOT NULL,
    project_id          TEXT,
    data                TEXT NOT NULL,
    created_by_agent_id TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_plugin_entities_type
    ON plugin_entities(plugin_id, entity_type);

CREATE INDEX IF NOT EXISTS idx_plugin_entities_project
    ON plugin_entities(project_id);

-- Polymorphic relation table: plugin<->plugin or plugin<->core entity.
-- `from_kind` / `to_kind` are "plugin" or "core"; `from_type` / `to_type`
-- are the entity type names (`work_item`, `project`, or manifest-declared).
CREATE TABLE IF NOT EXISTS plugin_entity_relations (
    id          TEXT PRIMARY KEY,
    from_kind   TEXT NOT NULL,
    from_type   TEXT NOT NULL,
    from_id     TEXT NOT NULL,
    to_kind     TEXT NOT NULL,
    to_type     TEXT NOT NULL,
    to_id       TEXT NOT NULL,
    relation    TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    UNIQUE(from_id, to_id, relation)
);

CREATE INDEX IF NOT EXISTS idx_plugin_entity_relations_from
    ON plugin_entity_relations(from_kind, from_type, from_id);

CREATE INDEX IF NOT EXISTS idx_plugin_entity_relations_to
    ON plugin_entity_relations(to_kind, to_type, to_id);

-- Persisted plugin workflow subscription bookkeeping. When a workflow using
-- a plugin-contributed trigger is enabled, core asks the plugin to subscribe
-- and records the returned subscription_id so it can re-bind on restart and
-- tear down on disable/delete.
CREATE TABLE IF NOT EXISTS plugin_workflow_subscriptions (
    id              TEXT PRIMARY KEY,
    plugin_id       TEXT NOT NULL,
    workflow_id     TEXT NOT NULL,
    trigger_kind    TEXT NOT NULL,
    subscription_id TEXT NOT NULL,
    config          TEXT NOT NULL DEFAULT '{}',
    created_at      TEXT NOT NULL,
    UNIQUE(plugin_id, workflow_id, trigger_kind)
);

CREATE INDEX IF NOT EXISTS idx_plugin_workflow_subs_workflow
    ON plugin_workflow_subscriptions(workflow_id);

CREATE INDEX IF NOT EXISTS idx_plugin_workflow_subs_plugin
    ON plugin_workflow_subscriptions(plugin_id);
