# Orbit Plugins — V1

Orbit plugins extend the app with custom **tools**, **entity types**, **OAuth providers**, **workflow triggers/nodes**, and **UI surfaces** — all shipped as a local zip file (or symlinked directory in dev mode). No remote registry in V1.

> **Status:** V1 backend foundation landed. The full author-facing docs
> (quickstart, manifest reference, MCP server guide, entity-types, core-api,
> oauth, packaging, security, INTERNAL_ARCHITECTURE, examples) are being
> written in a follow-up slice alongside the Node SDK and `create-orbit-plugin`
> CLI. See `plan.md` § "Plugin author documentation" for the full contents
> outline.

## Backend capabilities shipped

- Reverse-DNS plugin id + semver `hostApiVersion` range compat (currently `1.0.0`).
- Zip-slip-safe install staging and atomic commit.
- Dev-mode install from a directory (pointer file; gated on `developer.pluginDevMode`).
- `~/.orbit/plugins/registry.json` as the source of truth for installed state.
- Per-plugin Keychain namespace (`com.orbit.plugin.<id>`) with generic
  `store_secret` / `retrieve_secret` / `delete_secret` helpers.
- Manifest-declared tools bridged into the agent via `PluginToolHandler`
  (subprocess MCP transport coming online in the next slice).
- Auto-generated entity CRUD tool per manifest `entityTypes[]`, backed by the
  generic `plugin_entities` + `plugin_entity_relations` tables (migration
  `0024_plugin_entities.sql`).
- Deep-link scheme `orbit://oauth/callback` registered via
  `tauri-plugin-deep-link` on macOS, Windows, and Linux.
- `tauri-plugin-mcp-bridge` (used by external debugging agents) now gated
  behind the optional Cargo feature `debug-mcp-bridge`.

## Installing a plugin (once the UI lands)

1. Open the Plugins screen.
2. Install → pick `.zip` → review modal renders the manifest (tools, entity
   types, OAuth providers, workflow contributions, permissions).
3. Confirm → plugin lands in `~/.orbit/plugins/<id>/` disabled by default.
4. Flip the enable toggle → subprocess spawns lazily on first tool call.

## Developer mode

Set `developer.pluginDevMode = true` in `settings.json` to unlock dev
features:

- "Install from directory" (no zip required; symlink/pointer-installed)
- MCP JSON-RPC frame logging in the Live Log pane
- Relaxed zip size cap (default 50 MiB)
- Manifest hash skipping (manifests change every save in dev)

## Next slices

- Full MCP stdio client (subprocess spawn, `tools/list` round trip, progress
  notifications, graceful reload).
- OAuth browser launch + callback token exchange.
- Unix-socket core-API server that plugins call back into via
  `ORBIT_CORE_API_SOCKET`.
- `packages/plugin-sdk-node/` + `create-orbit-plugin` CLI.
- Bundled `com.orbit.github` flagship plugin.
- Plugins and Plugin Entities screens.
- Integration tests in `src-tauri/tests/plugins/`.
- Supabase mirror migration for `plugin_entities` + `plugin_entity_relations`.
