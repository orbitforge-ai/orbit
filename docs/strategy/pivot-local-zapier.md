# Pivot: Orbit as a Local-First Zapier

## Summary

Orbit is being repositioned from "agent IDE for macOS developers" to **"the local-first automation app for your Mac"** — a Zapier/n8n alternative that runs on your laptop, automates your desktop apps and APIs, and keeps your data local by default.

Tagline candidates:
- *"Automation that lives on your Mac."*
- *"Zapier for your computer, not the cloud."*
- *"Local-first automation."*

## Why this position

The ops-automation market is large and growing, but every meaningful competitor is cloud-only or server-oriented:

| Competitor | Hosting | Desktop integration | Per-task cost |
|---|---|---|---|
| Zapier | Cloud SaaS | None | Yes |
| Make (Integromat) | Cloud SaaS | None | Yes |
| Gumloop | Cloud SaaS | None | Yes |
| Lindy | Cloud SaaS | None | Yes |
| Relay.app | Cloud SaaS | None | Yes |
| n8n / Activepieces | Self-host server | None | No (self-host) |
| **Orbit** | **Local desktop app** | **Native macOS APIs** | **No** |

The **wedge none of them can match without rebuilding from scratch**: Orbit reads and writes the user's Mail.app, Apple Notes, Calendar, Reminders, Contacts, local files, clipboard, and screen — and triggers off macOS-native events (file watchers, hotkeys, app focus, AppleScript events, calendar events). The user's data never leaves their machine unless a workflow step explicitly sends it somewhere.

## Target user

A solo operator, freelancer, agency principal, or technical prosumer whose work happens on their Mac, not in 17 SaaS tabs. Examples: real-estate agent, accountant, lawyer, consultant, indie SaaS founder, agency owner, content creator, podcast producer, ecommerce operator.

Their pain is not "connect Stripe to Slack" (Zapier solves that). Their pain is "the messy reality of my desktop life is unautomatable" — invoices in Mail, notes in Apple Notes, contracts in `~/Documents`, calendar in Calendar, scripts in their terminal — and they don't want to put any of it in a third-party SaaS.

Secondary persona: technical operators (devs running side businesses, indie hackers, regulated solos) who want programmable workflows with their real shell, real CLIs, and Keychain-stored credentials.

## Strategic frame

Orbit's three load-bearing primitives carry over from the previous framing:

- **Entities** model the user's structured data (now: contacts, leads, invoices, tasks, events — not PRs and test runs).
- **Workflows** orchestrate the multi-step processes acting on that data.
- **Local-first execution** runs everything on the user's Mac.

The differentiator that *was* "git worktree-pinned agents" gets rebranded to **"isolated workflow runs"** — a workflow can be replayed against a snapshot of its input data without touching live state. Same primitive, broader appeal.

## What changes

### Primitives reordered

**Promoted to hero:**
- Workflow DAG engine (was infrastructure, now THE product).
- Trigger taxonomy (currently anemic — `trigger.manual` and `trigger.schedule` only — must become the deepest part of the stack).
- Visual workflow editor (moves from Tier 2 to Tier 1, day-one mandatory).
- Run history / observability (Zapier's "Task History" is what users debug from).

**Demoted but kept:**
- Worktree-pinning → reframed as "isolated workflow runs."
- Chat UI → secondary surface ("ask the workflow why it failed"), not the entry point.
- Sub-agents / planner-executor → reframed as "workflow step delegation."

**Dropped from public roadmap:**
- LSP plugin.
- Testkit plugin.
- PR/review entity model (GitHub plugin stays as one integration among many).
- All "agent IDE" framing in marketing copy.

### Architecture: macOS native is hybrid (core + bundled plugin)

The macOS-native capability layer cannot be a plugin because TCC entitlements are granted to the binary, not subprocesses. The integration layer can and should be a bundled, pre-enabled, first-party plugin so the plugin SDK gets battle-tested by the most important integration on the platform. This mirrors Raycast's architecture.

**In core (mandatory, non-pluggable):**
- TCC entitlements + permission prompts (Full Disk Access, Accessibility, Automation, Mail/Calendar/Contacts/Reminders).
- FSEvents file-watcher daemon.
- AppleScript / OSA runtime.
- Apple Shortcuts URL-scheme registration.
- Global hotkey registry.
- Notification Center integration.
- Spotlight-style launcher window.
- Menu bar / tray.

**As a bundled, pre-enabled, first-party plugin (`com.orbit.macos`):**
- Mail.app reading/sending.
- Calendar / Reminders / Contacts via EventKit.
- Apple Notes (AppleScript).
- Files (sits on top of core FSEvents).
- Shortcuts run/trigger nodes.

Critical files to modify for the entitlement layer:
- `src-tauri/tauri.conf.json` (currently no `macOS.bundle` config beyond minimal CSP).
- `src-tauri/capabilities/default.json` (currently only basic Tauri permissions).
- New: `src-tauri/entitlements.plist` and `src-tauri/Info.plist` overrides — macOS TCC keys (`NSMailUsageDescription`, `NSCalendarsUsageDescription`, `NSContactsUsageDescription`, `NSRemindersUsageDescription`, `NSAppleEventsUsageDescription`, `com.apple.security.automation.apple-events`, etc.) are not currently declared anywhere.

### Trigger taxonomy expansion

Today (`crates/orbit-engine/src/workflows/nodes/mod.rs`):
```rust
"trigger.manual" | "trigger.schedule" => Some(NodeExecutorKind::Trigger),
```

Plugin-extensible triggers exist via `WorkflowTriggerSpec` in `crates/orbit-engine/src/plugins/manifest.rs` (lines 208–219), pattern `trigger.{plugin_id}.{trigger_name}`. The infrastructure is right; the catalogue of triggers is empty.

Required new triggers (most as plugin-defined, some core):

Core:
- `trigger.webhook` — webhook ingress (with managed tunnel option for users who don't have a public URL).
- `trigger.hotkey` — global hotkey activation.
- `trigger.file` — FSEvents-backed file/directory watcher (created/modified/deleted/moved).
- `trigger.clipboard` — clipboard change.
- `trigger.app_focus` — macOS app activation/deactivation.

`com.orbit.macos` plugin triggers:
- `trigger.com_orbit_macos.mail_received` — new mail matching filter.
- `trigger.com_orbit_macos.calendar_event_starting` — N minutes before an event.
- `trigger.com_orbit_macos.reminder_due`.
- `trigger.com_orbit_macos.note_created` / `note_updated`.
- `trigger.com_orbit_macos.shortcut_run` — invoked from an Apple Shortcut.

Each plugin-bundled integration ships its own triggers (Stripe `payment_succeeded`, GitHub `pr_opened`, etc.).

### UI shifts (mandatory, not nice-to-have)

1. **Workflow canvas as the default screen.** First-run lands on a template gallery, not a chat window. (Today: chat is the entry point at `src/screens/Chat/`.)
2. **Trigger picker as first-class onboarding** — categories: Mail, Calendar, File, Schedule, Webhook, Hotkey, App Focus.
3. **Template gallery** — pre-built workflows installed in one click.
4. **Run history with replay** — every workflow run inspectable; "replay this run" button (where the rebranded isolated-run primitive pays off).
5. **Connection manager** — "Connected Apps" screen showing OAuth/auth status across plugins, à la Zapier.
6. **Spotlight-style global launcher** — `cmd+shift+space` opens a quick-trigger bar.

Chat UI moves to a secondary tab — kept for debugging and ad-hoc agent queries, not the primary surface.

### Bundled plugin priorities (the wedge)

In rough impact order:

1. **`com.orbit.macos`** — Mail.app, Apple Notes, Calendar, Reminders, Contacts, Files/FSEvents, Shortcuts, AppleScript bridge, notifications. **The single plugin that constitutes the competitive moat.**
2. **`com.orbit.web`** — Webhook ingress (with tunnel), HTTP request, scheduled fetch, web scrape.
3. **`com.orbit.gmail` / `com.orbit.gcal`** — for users not fully in Apple ecosystem.
4. **`com.orbit.stripe` / `com.orbit.quickbooks`** — money flows are killer-app territory for solo operators.
5. **`com.orbit.slack` / `com.orbit.discord`** — already bundled; reframe as "team comms triggers" not chat channels.
6. **`com.orbit.notion` / `com.orbit.airtable`** — bridge to where prosumers already keep structured data.
7. **`com.orbit.openai` / `com.orbit.anthropic` / `com.orbit.local-llm`** — LLM calls as workflow steps, not the only way to interact with Orbit.

Existing `com.orbit.github` plugin stays but becomes one integration among many, not the showcase.

## Phased roadmap

**Phase 1 — Trigger foundation + macOS native (the wedge).** Estimate: 2–3 months.
- Trigger SDK additions in plugin manifest (event types, filter expressions, debounce/dedupe).
- Core triggers: webhook, hotkey, file, clipboard, app_focus.
- TCC entitlements declared in `src-tauri/tauri.conf.json` + `entitlements.plist`.
- `com.orbit.macos` plugin: Mail, Notes, Calendar, Reminders, Contacts, Files, Shortcuts.
- Run history persistence + replay (extends existing `chat_sessions` and workflow tables).

**Phase 2 — Authoring UX.** Estimate: 2 months.
- Visual workflow canvas (React Flow). New screen: `src/screens/Workflows/`.
- Template gallery with one-click install.
- Connection manager screen.
- Dry-run / test-with-fixture mode (uses existing isolated-run primitive from `crates/orbit-engine/src/executor/session_worktree.rs`, generalized).

**Phase 3 — Distribution & ecosystem.** Estimate: 2 months.
- Plugin SDK polish (the third-party story; doc improvements and example plugins).
- Plugin marketplace (cloud-optional).
- Apple Shortcuts bridge in both directions.
- Browser extension companion ("trigger when on this page" / "save this page to entity").

**Phase 4 — Observability & power-user.** Estimate: 1.5 months.
- Workflow versioning + diff.
- Branching / parallel nodes (workflow engine v2 — currently `crates/orbit-engine/src/workflows/orchestrator.rs` rejects parallel branches at save time).
- Approval-gate node with desktop notification + reply.
- Per-workflow secrets & scopes.

## Pricing & distribution

Lean into Zapier's per-task metering as a sales weapon:

- **Free**: unlimited local workflows, 3 active triggers, watermark on shared templates.
- **Pro** (~$15/mo or $99 one-time + cheap upgrades): unlimited triggers, cloud sync, managed tunnel for webhook ingress, template marketplace.
- **Team**: shared workflow library, multi-machine sync.

Distribution: notarized DMG via DTC; Mac App Store eventually (some entitlements are easier outside the Store — evaluate per-feature).

## Marketing & positioning

- **Hero visuals**: a workflow canvas with macOS app icons (Mail, Calendar, Notes) wired together — not chat bubbles.
- **Comparison page**: literal table vs Zapier/Make/n8n highlighting macOS access, local data, no per-task cost.
- **Content strategy**: "How to automate X on your Mac" — invoice processing, lead intake, content publishing, podcast prep, real-estate listings. Each blog post = a downloadable template.
- **Killer demo**: An invoice arrives in Mail.app → Orbit extracts amount and client → checks local CSV/Numbers for matching project → drafts a reply in Mail → adds an entry to a tracking spreadsheet → schedules a calendar follow-up if unpaid in 14 days. **Zero steps involve a third-party SaaS.** No competitor can do this end-to-end.

## What stays unchanged

- Plugin SDK architecture (MCP-stdio with manifests at `crates/orbit-engine/src/plugins/manifest.rs`) — already perfect for third-party integration builders.
- SQLite + Supabase storage model — entities map cleanly to "leads," "invoices," "tasks."
- Mem0 integration (`crates/orbit-engine/src/memory_service.rs`) — workflows that "remember" what they did last time is a real Zapier weakness.
- Tauri macOS shell — it *is* the wedge.
- Cron scheduler — already powers `trigger.schedule`.
- Workflow DAG engine (`crates/orbit-engine/src/workflows/orchestrator.rs`) — already there, just gets a UI and more triggers.

Estimate: ~60% of the engineering from the prior dev-IDE roadmap carries over (workflow engine, entity model, plugin SDK, memory, storage). The new ~40% is concentrated in (a) the macOS native plugin + entitlements, (b) trigger taxonomy + run history, and (c) the visual authoring UI. None of it is technically novel — execution work, not research.

## What we explicitly do NOT do

- Build mobile clients (that's OpenClaw's product).
- Add voice or wake-word features.
- Add 20+ chat channels (Slack + Discord + Teams is enough).
- Add 30+ providers (Anthropic + OpenAI + local-LLM fallback covers the workflow-step needs).
- Build a Canvas/A2UI clone (workflow canvas + entity views > arbitrary HTML).
- Compete head-to-head with Cursor/Zed/Windsurf on agent IDE features.

## Critical files referenced

- `src-tauri/tauri.conf.json` — needs macOS bundle config + entitlements declarations.
- `src-tauri/capabilities/default.json` — current Tauri permissions; needs expansion.
- New: `src-tauri/entitlements.plist`, `src-tauri/Info.plist` overrides — TCC keys.
- `crates/orbit-engine/src/workflows/nodes/mod.rs` — trigger registry, currently 2 entries.
- `crates/orbit-engine/src/workflows/orchestrator.rs` — workflow engine; needs v2 parallel-branch support in Phase 4.
- `crates/orbit-engine/src/plugins/manifest.rs` — `WorkflowTriggerSpec` already plugin-extensible.
- `crates/orbit-engine/src/executor/session_worktree.rs` — generalize from "session-scoped worktree" to "isolated workflow run snapshot."
- `crates/orbit-engine/src/memory_service.rs` — Mem0 integration; minor scoping additions for workflow-context memory.
- `bundled-plugins/` — new plugins land here; `com.orbit.macos` is highest priority.
- `src/screens/Chat/` — demote from default screen.
- New: `src/screens/Workflows/` — the new default screen with canvas and template gallery.

## Verification

This document is a strategic position, not a code change. Verification of the pivot lands in Phase 1 deliverables:

- TCC entitlements declared and the app prompts correctly for Mail/Calendar/Contacts on first relevant workflow.
- A workflow with a `trigger.com_orbit_macos.mail_received` trigger fires when a real message arrives in Mail.app on the dev's machine.
- A workflow with a `trigger.file` trigger fires when a file lands in `~/Downloads/`.
- The workflow canvas renders, allows wiring a trigger → agent.run → integration.* node, saves it, and runs successfully.
- Run history shows the run with input data, node outputs, and a working replay button.
- Killer demo (invoice → reply → spreadsheet → calendar follow-up) executes end-to-end on a fresh Mac.

Subsequent plans will define each phase's deliverables in code-level detail. This document is the umbrella positioning all of them roll up to.
