# Packaging

A plugin zip is just a flat archive of the plugin directory with `plugin.json` at the root.

## Layout

```
my-plugin.zip
├── plugin.json
├── server.js          (or whatever entry the manifest names)
├── package.json       (for Node plugins)
├── node_modules/      (bundle your deps — the subprocess runs `env_clear`)
├── icon.png           (optional)
└── README.md          (optional)
```

## Size cap

V1 enforces a 50 MiB zip size cap on install (relaxed for dev-mode installs).

## Safety

- Zip-slip is rejected at extract — any entry whose normalised path escapes the root fails the install.
- `plugin.json` parse errors fail the install.
- `hostApiVersion` mismatch fails with a clear message naming the needed host version.

## Distribute

V1 has no registry. Ship the zip via GitHub releases, a personal site, or your chat of choice. Users install with **Plugins** → **Install from file**.

## Version bumps

- Bump `version` in `plugin.json` on every release (semver).
- Breaking changes to entity schemas should bump the minor or major version; V1 entity storage is schema-advisory, so additive changes are safe to ship as patch releases.
- Changing `id` is a hard break — it's treated as a brand-new plugin, and users will see both the old and new cards side-by-side.

## CI

For plugin authors: validate + pack in CI with the tool shipped by `@orbit/plugin-sdk` (follow-up: `@orbit/plugin-tools`). Until that ships, a plain `zip -r my-plugin.zip my-plugin` is fine.
