# Entity types

Plugins can declare new record types — structured JSON blobs with schemas, relations, and auto-generated agent tools. Use this when your plugin owns data that users + agents will operate on (social media posts, candidate profiles, anything that fits the "typed card" pattern).

## Declare

```jsonc
"entityTypes": [
  {
    "name": "content",
    "displayName": "Content",
    "icon": "file-text",
    "schema": {
      "type": "object",
      "required": ["text", "platform"],
      "properties": {
        "platform": { "type": "string", "enum": ["twitter", "bluesky", "linkedin"] },
        "text": { "type": "string", "maxLength": 500 },
        "scheduledFor": { "type": "string", "format": "date-time" },
        "status": { "type": "string", "enum": ["draft", "scheduled", "posted"] }
      }
    },
    "relations": [
      { "name": "parent_work_item", "to": "work_item", "cardinality": "one" },
      { "name": "replies_to", "to": "content", "cardinality": "one" }
    ],
    "listFields": ["platform", "status", "scheduledFor"],
    "titleField": "text",
    "indexedFields": ["status", "scheduledFor"]
  }
]
```

## Auto-generated CRUD

For every declared entity type, Orbit exposes a single agent tool: `<plugin-id-slug>__<entity-type>`. Actions:

- `list` → `{ projectId?, limit?, offset? }` → `{ items: [...] }`
- `get` → `{ id }` → entity
- `create` → `{ data, projectId? }` → entity
- `update` → `{ id, data }` → entity
- `delete` → `{ id }`
- `link` → `{ id, relation, toKind, toType, toId }`
- `unlink` → `{ id, toId, relation }`
- `list_relations` → `{ id }` → `[{ ...relation }]`

The JSON Schema from the manifest is pulled into the `data` field's schema, so agents get full validation from the LLM side.

## Storage

Two generic tables: `plugin_entities` (id, plugin_id, entity_type, project_id, data JSON, timestamps) and `plugin_entity_relations` (polymorphic). Core-owned. Plugins never ship SQL.

## Uninstall policy

On disable, rows are retained. On uninstall, rows are **kept** (default) with a banner in the Plugin Entities screen offering a "Purge data" action. This matches the cloud-orphan behavior so reinstalling a plugin rehydrates its data.

## Plugin access from your MCP subprocess

Use the [`core` client from the SDK](core-api.md):

```ts
plugin.tool('schedule', {
  inputSchema: { type: 'object', required: ['contentId'], properties: { contentId: { type: 'string' } } },
  run: async ({ input, core }) => {
    const content = await core.entity.get((input as any).contentId);
    if (!content) throw new Error('not found');
    await core.entity.update(content.id, { ...content.data, status: 'scheduled' });
    return 'ok';
  },
});
```

## Frontend rendering

V1 uses generic components keyed off `listFields`, `titleField`, `icon`. The Plugin Entities screen lists every plugin entity type with filter/search. No plugin React loads in the renderer — custom rendering is a V1.1 topic.
