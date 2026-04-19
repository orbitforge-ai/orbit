# Internal architecture

Reference for contributors (including AI agents) extending the plugin system itself. Read this before changing anything under `src-tauri/src/plugins/`.

## Module map

| File | Responsibility | Public types |
|---|---|---|
| `plugins/mod.rs` | `PluginManager` + Tauri state glue | `PluginManager`, `PluginSummary`, `PLUGIN_HOST_API_VERSION` |
| `plugins/manifest.rs` | Manifest parse + validate | `PluginManifest`, `*Spec` structs |
| `plugins/registry.rs` | `registry.json` load/save | `PluginRegistry`, `RegistryEntry` |
| `plugins/install.rs` | Zip + directory install, zip-slip defense | `stage_from_zip`, `commit_from_staging`, `install_from_directory` |
| `plugins/mcp_client.rs` | Low-level MCP stdio client | `McpClient`, `LaunchSpec` |
| `plugins/runtime.rs` | Subprocess lifecycle + log ring + per-plugin mutex | `RuntimeRegistry`, `RuntimeStatus` |
| `plugins/entities.rs` | DB CRUD for `plugin_entities` / `plugin_entity_relations` | `PluginEntity`, `ListFilter`, free functions |
| `plugins/core_api.rs` | Unix-socket JSON-RPC server | `CoreApiServer` |
| `plugins/oauth.rs` | PKCE + callback + Keychain wrapping + env injection | `OAuthState`, `start_flow`, `handle_callback`, `build_env_for_subprocess` |
| `plugins/hooks.rs` | Hook event bus | `HookEvent`, `HookOutcome`, `fire` |
| `plugins/tools.rs` | `PluginToolHandler`, `EntityToolHandler`, handler builder | |
| `commands/plugins.rs` | Tauri command handlers | |
| `executor/tools/plugin_management.rs` | Agent-facing plugin lifecycle tool | `PluginManagementTool` |
| `executor/builtin_skills/create-plugin/SKILL.md` | Built-in skill teaching agents to scaffold plugins | |

## Data flows

### Tool call
```
agent → execute_tool (agent_tools.rs)
     → is_plugin_tool_name("slug__name")?
     → plugins::from_state(app)
     → PluginToolHandler.execute
     → manager.runtime.call_tool(manifest, name, args, env)
     → lazy spawn subprocess via mcp_client
     → JSON-RPC over stdio: tools/call
     → response → agent
```

### Entity CRUD (auto-generated)
```
agent → execute_tool
     → EntityToolHandler.execute
     → plugins::entities::create/update/list/...
     → plugin_entities table
     → emit plugin:entity:changed
```

### Hook fire
```
core event (work_item completed, session started, etc.)
     → hooks::fire(event, payload, manager)
     → filter to subscribed plugins
     → (future) runtime.dispatch_notification to each subprocess
```

### UI click (sidebar / chat action / slash command)
```
React component (PluginSidebarItem, etc.)
     → invoke('list_plugin_entities', ...) or tool-dispatch via agent
     → Tauri command → PluginManager
     → either DB read or subprocess call via runtime.call_tool
     → rendered result (markdown | text | form)
```

## Extensibility recipes

### Add a new hook event

1. Add a variant to `HookEvent` in `plugins/hooks.rs` + matching `as_str()` arm.
2. Emit with `hooks::fire(&manager, HookEvent::MyEvent, &payload)` at the appropriate call site (session lifecycle → `session_agent.rs`; tool calls → `agent_loop.rs`; entity writes → `entities.rs` or the existing core tool files).
3. List the new event in `docs/plugins/manifest-reference.md#hooks`.
4. Add a test that registers a subscriber and asserts fire.

### Add a new UI extension point type

1. Extend the manifest `ui` block in `plugins/manifest.rs` (new array field on `UiSpec`).
2. Update `plugin.schema.json`; the TypeScript types in `packages/plugin-sdk-node/` are generated from it.
3. Add a `list_plugin_ui_<type>` Tauri command in `commands/plugins.rs`.
4. Add a React component under `src/components/plugins/`.
5. Wire it into the host surface (e.g., sidebar for sidebar items, chat toolbar for actions).
6. Surface the contribution in the install review modal (`src/screens/Plugins/PluginInstallModal.tsx`).

### Add a new core-API method (plugin → core)

1. Add a match arm in `plugins/core_api.rs::dispatch`.
2. Add a typed wrapper in `packages/plugin-sdk-node/src/core.ts`.
3. If the method reads core entities, gate it on `permissions.coreEntities`.
4. Document it in `docs/plugins/core-api.md`.

### Add a new manifest field

1. Add the field to the appropriate `*Spec` struct in `plugins/manifest.rs`.
2. Update `plugin.schema.json`.
3. Update the TypeScript types in `src/api/plugins.ts` (`PluginManifest`) and the SDK's `plugin-sdk-node/src/`.
4. Display it in the install review modal if user-visible.
5. Document it in `manifest-reference.md`.

### Add a new Tauri command

1. Add handler in `src-tauri/src/commands/plugins.rs`.
2. Register in `invoke_handler!` in `src-tauri/src/lib.rs`.
3. Add wrapper method in `src/api/plugins.ts`.
4. Use from the Plugins screen or detail drawer.

## Invariants

- Plugin tool names are always `<plugin-id-slug>__<tool-name>`. `slugify_id` replaces `.` and `-` with `_`.
- Plugin JS is **never** loaded into the Orbit renderer in V1. UI contributions are declarative.
- Every plugin tool call goes through `classify_tool_call` → `RiskLevel::Prompt`. Never auto-allowed.
- Keychain service names are always `com.orbit.plugin.<plugin-id>`.
- `plugin_entities.data` is opaque JSON; typed access goes through manifest JSON Schema validation at write time.
- Plugin subprocess env **never** inherits the user's shell env. `env_clear` is applied before launch; only explicit vars are injected.
- Per-plugin core-API socket path: `~/.orbit/plugins/<id>/.orbit/core.sock`, 0600 permissions.

## Testing patterns

Integration tests live in `src-tauri/tests/plugins/`. Each test spins up a stub MCP server (a small Node or Rust binary that responds to `initialize` / `tools/list` / `tools/call`) and drives the `PluginManager` through its full lifecycle.

Unit tests in `plugins/*.rs` cover manifest validation, registry round-trips, zip-slip rejection, PKCE generation, and the entity CRUD functions against a temp-DB.

## Common failure modes

| Symptom | Likely cause |
|---|---|
| Plugin subprocess exits immediately | `runtime.command` not in PATH, or the plugin's `server.js` throws before reading stdin. Check `plugin:log:<id>` event stream. |
| Agent doesn't see the plugin's tools | Plugin disabled, or `plugins:changed` event dropped. Refresh the agent's tool definitions. |
| OAuth callback returns 404 | Deep link scheme not registered — confirm `tauri.conf.json` has `plugins.deep-link.desktop.schemes = ["orbit"]`. |
| `tools/call` times out | The subprocess isn't writing its response back on stdout. Check the log ring. |
| Plugin can't read a core entity | Missing from `permissions.coreEntities`. |

## Change log

Append one line per substantive change: `YYYY-MM-DD — short summary; see <file>:<line>`. Keeps this doc alive.
