# `plugin.json` reference

One object. Every field is documented below. For editor validation, add `"$schema": "https://orbit.dev/plugins/plugin.schema.json"` (or the local path to `docs/plugins/plugin.schema.json`).

## Top-level

| Field | Type | Required | Notes |
|---|---|---|---|
| `schemaVersion` | `integer` | ✓ | Must be `1` in V1. |
| `hostApiVersion` | `string` | ✓ | Semver range. V1 Orbit's host API is `1.0.0`; declare `"^1.0.0"` to accept future 1.x hosts. |
| `id` | `string` | ✓ | Reverse-DNS, e.g. `com.orbit.github`. Unique per device. |
| `name` | `string` | ✓ | Human display name. |
| `version` | `string` | ✓ | Semver. |
| `description` | `string` | | Shown in the install modal + detail drawer. |
| `author` | `string` | | |
| `homepage` | `string` | | URL. |
| `license` | `string` | | SPDX id. |
| `icon` | `string` | | Path relative to plugin root. |
| `runtime` | `object` | ✓ | See [Runtime](#runtime). |
| `tools` | `array<object>` | | See [Tools](#tools). |
| `entityTypes` | `array<object>` | | See [Entity types](entity-types.md). |
| `oauthProviders` | `array<object>` | | See [OAuth](oauth.md). |
| `permissions` | `object` | | See [Permissions](#permissions). |
| `hooks` | `object` | | See [Hooks](#hooks). |
| `workflow` | `object` | | See [Workflow](#workflow). |
| `ui` | `object` | | See [UI extension points](#ui). |

## Runtime

```jsonc
"runtime": {
  "type": "mcp-stdio",           // only value in V1
  "command": "node",             // binary to spawn (must be in PATH or relative to plugin dir)
  "args": ["server.js"],
  "workingDir": ".",             // relative to plugin dir
  "env": { "NODE_OPTIONS": "--max-old-space-size=512" }
}
```

Orbit calls `env_clear` on the subprocess before launch. PATH and HOME are injected back in; everything else is explicit. See `oauth.md` for how OAuth tokens are passed.

## Tools

```jsonc
"tools": [
  {
    "name": "clone_repo",
    "description": "Clone a GitHub repo.",
    "riskLevel": "moderate",    // safe | moderate | dangerous
    "inputSchema": {
      "type": "object",
      "required": ["repo"],
      "properties": { "repo": { "type": "string" } }
    }
  }
]
```

Every tool call is permission-prompted. `riskLevel` influences the prompt; it never auto-allows (that's a V1 policy). `name` must not contain `__` — that's reserved as the plugin-namespace separator. Agents see the tool as `<plugin-id-slug>__<tool-name>`.

## Permissions

```jsonc
"permissions": {
  "network": ["api.github.com"],       // advisory — not enforced in V1
  "filesystem": ["workspace"],         // advisory
  "oauth": ["github"],                 // must list every provider id
  "coreEntities": ["work_item"]        // whitelist of core entities the plugin can read via core-api
}
```

`coreEntities` is enforced — the core-API socket rejects reads of entities not in this list.

## Hooks

```jsonc
"hooks": {
  "subscribe": [
    "session.started",
    "agent.tool.before_call",
    "entity.work_item.after_complete"
  ]
}
```

Fixed V1 catalog:

| Event | Blocking? | Notes |
|---|---|---|
| `session.started`, `session.ended` | no | |
| `agent.tool.before_call` | yes | Return `{veto: true, reason}` to deny. |
| `agent.tool.after_call` | no | |
| `entity.work_item.after_create` | no | |
| `entity.work_item.after_complete` | no | |
| `entity.work_item.before_delete` | yes | |
| `oauth.connected` | no | This plugin's own OAuth. |
| `plugin.enabled`, `plugin.disabled` | no | |

Blocking hooks have a 2-second timeout; missing or errored responses are treated as "no opinion".

## Workflow

```jsonc
"workflow": {
  "triggers": [
    {
      "kind": "trigger.com_orbit_slack.incoming_message",
      "displayName": "Incoming Slack message",
      "configSchema": { ... },
      "outputSchema": { ... },
      "subscriptionTool": "subscribe_incoming_message"
    }
  ],
  "nodes": [
    {
      "kind": "integration.com_orbit_social.post",
      "displayName": "Post to social",
      "tool": "post_from_workflow",
      "inputSchema": { ... },
      "outputSchema": { ... }
    }
  ]
}
```

Trigger `kind` must start with `trigger.<slug>.` and node `kind` with `integration.<slug>.`, where `<slug>` is `slugify(id)` (dots + dashes → underscores). The validator rejects mismatches.

## UI

```jsonc
"ui": {
  "sidebarItems": [
    { "id": "content-queue", "label": "Content Queue", "icon": "send", "view": "entity-list:content" }
  ],
  "entityDetailTabs": [
    { "targetEntity": "work_item", "id": "social-preview", "label": "Social preview", "renderTool": "render_preview" }
  ],
  "agentChatActions": [
    { "id": "schedule-post", "label": "Schedule as post", "tool": "schedule_from_message" }
  ],
  "slashCommands": [
    { "name": "/post", "description": "Schedule a social post", "tool": "quick_post" }
  ],
  "settingsPanels": [
    { "id": "social-defaults", "label": "Social defaults", "renderTool": "render_settings" }
  ]
}
```

V1 renders these declaratively — no plugin JavaScript loads in the renderer. `renderTool` returns `{ type: "markdown" | "text" | "form", content }`. See [`INTERNAL_ARCHITECTURE.md`](INTERNAL_ARCHITECTURE.md) for how to add a new UI extension point type.
