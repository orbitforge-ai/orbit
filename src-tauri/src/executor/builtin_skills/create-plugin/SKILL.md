---
name: create-plugin
description: Scaffold, implement, and install a new Orbit plugin — custom tools, entity types, workflow contributions, and OAuth integrations. Use when the user asks to build an integration (e.g., GitHub, Slack, custom entity type) that doesn't exist yet, or wants to wrap a script as an Orbit plugin.
---

# Create Plugin

## When to use

The user wants a new integration or wants to wrap local logic as a reusable Orbit plugin. Examples:
- "Add a GitHub plugin that can clone repos and open PRs"
- "I want a plugin that stores social media posts as a new entity type"
- "Wrap this node script as an Orbit tool"

## Process

### 1. Clarify scope (use `ask_user`)
Ask the user:
- What does the plugin do at a high level?
- Does it need OAuth? If so, which provider? PKCE or confidential client (classic GitHub OAuth App)?
- Does it introduce a new entity type with its own JSON Schema?
- What tools should agents be able to call?
- Does it need to contribute workflow triggers or action nodes?

### 2. Pick a template and scaffold
Use `shell_command` to run the scaffolding CLI — writes `plugin.json`, `server.js`, `package.json`, `README.md`:

```bash
npx create-orbit-plugin <name> --template=<node-tool-only|node-entity|node-oauth>
```

Templates:
- `node-tool-only` — plain tool server, no entity types, no OAuth
- `node-entity` — entity type + auto-generated CRUD + a behavior tool that reads entities
- `node-oauth` — OAuth provider (PKCE or confidential) + a tool that uses the token

### 3. Read and review the generated files
Use `read_file` on `plugin.json` and `server.js`. Confirm the `id` is reverse-DNS (`com.<author>.<name>`), the `hostApiVersion` is `^1.0.0`, and the shape matches user intent.

### 4. Fill in the manifest
Edit `plugin.json`:
- `id`, `name`, `description`, `author`, `version` (semver)
- `runtime`: `type: "mcp-stdio"`, `command`, `args`
- `tools[]`: each with `name`, `description`, `riskLevel` (`safe|moderate|dangerous`), optional `inputSchema`
- `entityTypes[]` (if any): JSON Schema in `schema`, `listFields`, `titleField`, optional `indexedFields`, `relations[]` to other plugin entities or core entities
- `oauthProviders[]` (if any): `id`, `authorizationUrl`, `tokenUrl`, `scopes`, `clientType`
- `permissions`: `network`, `oauth`, `coreEntities` (whitelist of core entities plugin can read — `work_item`, `project`, etc.)
- `hooks.subscribe[]` (optional): `session.started`, `agent.tool.before_call`, `entity.work_item.after_complete`, etc.
- `workflow.triggers[]` / `workflow.nodes[]` (optional): prefix `trigger.<slug>.` / `integration.<slug>.`

Reference: `docs/plugins/manifest-reference.md` and the JSON Schema at `docs/plugins/plugin.schema.json`.

### 5. Implement the tools
Edit `server.js` — one `plugin.tool(name, { inputSchema, run })` per declared tool. Inside `run`, use:
- `core.entity.*` to read/write this plugin's entity types (round-trips over `ORBIT_CORE_API_SOCKET`)
- `oauth.<providerId>.accessToken` for OAuth calls (read from `ORBIT_OAUTH_<PROVIDER>_ACCESS_TOKEN`)
- `progress(fraction, message)` to stream updates back to the agent
- `log(line)` for debug output (appears in the Plugin detail drawer's Live Log tab)

### 6. Dev install
Call `plugin_management`:
```json
{ "action": "install_from_directory", "path": "<workspace>/<plugin-name>" }
```
The plugin appears in the Plugins screen as disabled, `dev: true`.

### 7. Enable and test
```json
{ "action": "enable", "plugin_id": "<id>" }
```
Then call one of the plugin's namespaced tools via the normal agent tool flow (e.g., `com_orbit_hello__greet`). Confirm the return value.

### 8. Iterate
After each edit to `plugin.json` or `server.js`:
```json
{ "action": "reload", "plugin_id": "<id>" }
```
Subprocess is killed; next tool call respawns with the updated manifest.

### 9. Summarize
Tell the user:
- Where the plugin lives (`<workspace>/<name>/`)
- Its id, version, and enabled state
- Every exposed tool name (namespaced) and entity type
- OAuth provider(s) and how to connect them (open the Plugins screen → Configure → Connect)
- How to package for distribution: `npx @orbit/plugin-tools pack`

## Patterns worth copying

- `docs/plugins/examples/hello-world/` — minimal tool-only plugin
- `docs/plugins/examples/social-content/` — entity type + relations + behavior tools
- `docs/plugins/examples/github-oauth/` — confidential-client OAuth with classic GitHub App

## Common pitfalls

- `id` must be reverse-DNS (`com.author.name`); must be unique on the device. Changing `id` is treated as a new plugin.
- Tool `name` must not contain `__` (reserved as the plugin-namespace separator — the agent sees `<slug>__<tool-name>`).
- `redirectUri` in every OAuth provider must be exactly `http://127.0.0.1:47821/oauth/callback` (Orbit runs a loopback HTTP listener on that port; RFC 8252 § 7.3).
- `runtime.command` must exist in `PATH` or be a relative path inside the plugin directory. `env_clear` is applied before launch — PATH and HOME are re-injected, but the user's shell rc files are not.
- Manifest `hostApiVersion` must satisfy the current Orbit build (`1.0.0` in V1). Declare `"hostApiVersion": "^1.0.0"` to match future 1.x hosts.
- JSON-Schema `required` fields in entity schemas must actually be filled by the `create` action — missing fields are rejected at the manifest validator (coming from the SDK's helper).

## Verification checklist

After install + enable, confirm:
1. Plugin appears in the Plugins screen (status dot green after first tool call).
2. `plugin_management { "action": "status", "plugin_id": "<id>" }` returns `running: true`.
3. `plugin_management { "action": "logs", "plugin_id": "<id>" }` shows subprocess stderr (helpful for debugging startup failures).
4. At least one declared tool round-trips when the agent calls it.
5. If the plugin declares an OAuth provider: `plugin_management { "action": "oauth_status", "plugin_id": "<id>" }` shows the provider. User completes Connect → the field flips to `connected: true`.
6. If the plugin declares entity types: create one via the auto-generated `<slug>__<entity-type>` tool and confirm it appears in the Plugin Entities screen.
