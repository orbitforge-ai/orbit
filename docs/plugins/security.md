# Security

What V1 does and does not protect against.

## In scope

- **Zip-slip defense** — archive entries cannot escape the extraction root.
- **Manifest host-API check** — incompatible plugins are rejected at install with a clear error.
- **Disabled-by-default** — a freshly installed plugin never runs until the user enables it.
- **Per-call permission prompt** — every plugin tool invocation is prompted unless the user has saved an explicit "always allow" rule for that tool. Plugin tools never auto-allow.
- **Subprocess isolation** — plugins run as separate OS processes. `kill_on_drop` enforces reap on parent exit.
- **Env isolation** — the subprocess receives only the env vars Orbit explicitly injects (OAuth tokens, the core-api socket path, `PATH`, `HOME`). The user's shell rc files are never sourced.
- **Keychain-scoped secrets** — OAuth tokens and client secrets live under `com.orbit.plugin.<id>`, never on disk in plaintext.
- **Core-entity allowlist** — a plugin can only read core entities (`work_item`, etc.) that its manifest explicitly whitelists in `permissions.coreEntities`.
- **Per-plugin core-API socket** — the unix socket is bound at `~/.orbit/plugins/<id>/.orbit/core.sock` with 0600 permissions, restricting access to that plugin's subprocess.
- **Deep-link state TTL** — OAuth flow state entries expire after 10 minutes.

## Out of scope (V2+)

- **Code signing** — plugin authors are trusted; no signing or signature verification.
- **Sandbox entitlements** — plugins run with the same OS-level privileges as Orbit itself. If a plugin reads your filesystem or makes network calls, the OS does not stop it.
- **Network egress enforcement** — the `permissions.network` manifest field is advisory only, displayed to the user at install but not enforced at runtime.
- **Filesystem jailing** — `permissions.filesystem` is advisory.
- **Resource limits** — no CPU or memory caps on the subprocess.
- **Auto-update** — no update checker; authors ship new zips manually.
- **Plugin JavaScript in the renderer** — V1 never loads plugin JS into the Orbit UI. All UI extensions are declarative (see [`manifest-reference.md#ui`](manifest-reference.md#ui)).

## Trust model

Plugins are user-authorized integrations. Treat them like any other third-party binary you install locally — the security boundary is the Orbit install step (which surfaces the manifest and its declared contributions) and the per-call permission prompt. If a plugin misbehaves, disable or uninstall it.
