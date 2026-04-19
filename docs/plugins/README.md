# Orbit Plugins

Orbit plugins are **local-install integrations** that add custom tools, entity types, OAuth providers, workflow triggers/nodes, and declarative UI surfaces to the Orbit desktop app. Plugins run as MCP stdio subprocesses owned by their plugin author — no plugin JavaScript runs inside Orbit's renderer.

## Quick links

- [Quickstart](quickstart.md) — build and install a hello-world plugin in 10 minutes
- [Manifest reference](manifest-reference.md) — every `plugin.json` field
- [MCP server guide](mcp-server.md) — the protocol Orbit speaks to your plugin
- [Entity types](entity-types.md) — declare custom entity types and relations
- [Core API](core-api.md) — the methods plugins call back into Orbit
- [OAuth](oauth.md) — PKCE + confidential-client flows
- [Packaging](packaging.md) — how to ship a plugin zip
- [Security](security.md) — what Orbit guarantees vs what plugins are trusted with
- [Internal architecture](INTERNAL_ARCHITECTURE.md) — reference for contributors extending the plugin system itself
- [JSON Schema](plugin.schema.json) — for editor validation; reference from `plugin.json` via `"$schema"`

## Worked examples

- [hello-world](examples/hello-world/) — minimal tool-only plugin
- [social-content](examples/social-content/) — entity type + relations + behavior tools
- [github-oauth](examples/github-oauth/) — confidential-client OAuth

## What a plugin can do

- **Tools** — agents call them directly (namespaced `<plugin-id-slug>__<tool-name>`).
- **Entity types** — structured records agents can CRUD via auto-generated tools. Stored as JSON blobs in Orbit's SQLite; validated against the manifest JSON Schema.
- **OAuth providers** — tokens stored in the macOS Keychain, delivered to the subprocess as `ORBIT_OAUTH_<PROVIDER>_ACCESS_TOKEN`.
- **Workflow triggers + action nodes** — extend the workflow editor with custom event sources and action steps.
- **Hook subscriptions** — get notified of core events (`session.started`, `entity.work_item.after_complete`, etc.).
- **Declarative UI** — sidebar items, entity detail tabs, slash commands, settings panels. All rendered by Orbit; plugin logic stays in the MCP subprocess.

## Host API version

V1 host API is **`1.0.0`**. Declare compatibility with `"hostApiVersion": "^1.0.0"` in your manifest. Plugins whose declared range is unsatisfied by the current Orbit build are rejected at install time.
