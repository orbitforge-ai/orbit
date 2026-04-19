# Core API

Your plugin's MCP subprocess can call back into Orbit over a unix-domain socket at `ORBIT_CORE_API_SOCKET`. Newline-delimited JSON-RPC 2.0 requests in, responses out. Each request carries an `id` so interleaved calls are safe.

## Via the SDK

```ts
import { Plugin } from '@orbit/plugin-sdk';

plugin.tool('create_post', {
  run: async ({ input, core }) => {
    const content = await core.entity.create('content', input);
    await core.entity.link('content', content.id, 'parent_work_item', {
      kind: 'core',
      type: 'work_item',
      id: (input as any).workItemId,
    });
    return content.id;
  },
});
```

## Raw JSON-RPC

```json
{"jsonrpc": "2.0", "id": 1, "method": "entity.create",
 "params": {"entityType": "note", "data": {"title": "hi"}}}
```

## Methods

| Method | Params | Returns |
|---|---|---|
| `entity.list` | `{entityType, projectId?, limit?, offset?}` | `[entity, ...]` |
| `entity.get` | `{id}` | `entity` or `null` |
| `entity.create` | `{entityType, data, projectId?}` | `entity` |
| `entity.update` | `{id, data}` | `entity` |
| `entity.delete` | `{id}` | `{deleted: id}` |
| `entity.link` | `{fromType, fromId, toKind, toType, toId, relation}` | relation |
| `entity.unlink` | `{fromId, toId, relation}` | `{unlinked: true}` |
| `entity.list_relations` | `{id}` | `[relation, ...]` |
| `work_item.get` | `{id}` | core entity (requires `coreEntities: ["work_item"]`) |
| `work_item.list` | `{projectId}` | `{items: [...]}` (requires `coreEntities: ["work_item"]`) |
| `workflow.fire_trigger` | `{kind, payload, dedupeKey?}` | `{accepted: true}` |

## Authorization

- Entity methods operate only on your own plugin's entities. A request with a mismatched `plugin_id` (carried implicitly by the socket path) is rejected.
- Core entity reads (`work_item.*`) require an explicit whitelist in `permissions.coreEntities`. Default denies.
- The socket is bound with 0600 permissions under `~/.orbit/plugins/<id>/.orbit/core.sock`, so only the plugin's own subprocess can dial in.

## `workflow.fire_trigger`

Plugins that declare workflow triggers in their manifest call this when an external event arrives (webhook, websocket message, poll hit). Orbit looks up every enabled workflow with a matching `trigger_kind` and starts a run via the standard `WorkflowOrchestrator::start_run` path.

```json
{
  "method": "workflow.fire_trigger",
  "params": {
    "kind": "trigger.com_orbit_slack.incoming_message",
    "dedupeKey": "<msg-ts>",
    "payload": { "text": "hi", "userId": "U123", "timestamp": "..." }
  }
}
```

`dedupeKey` is optional; duplicates within 5 minutes are skipped.
