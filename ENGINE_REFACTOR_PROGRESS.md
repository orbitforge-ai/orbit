# Phase Progress — Engine extraction + per-backend repo traits

Source plan: [`/Users/matwaroff/.claude/plans/quirky-jingling-candy.md`](/Users/matwaroff/.claude/plans/quirky-jingling-candy.md)
Council edits: `/Users/matwaroff/.claude/plans/please-review-this-plan-elegant-meteor.md`

This is a living index of what's done in Phase B so work can be split across agents without overlap. Update it as you finish slices. Keep entries terse — one line each.

---

## Status legend

- `[done]` merged on this branch (`feat/extract-engine`)
- `[wip]` someone is actively working on it (claim by editing this file)
- `[next]` ready to pick up; preconditions met
- `[blocked]` waiting on another slice; note which one. Recheck if still blocked if next in line.
- `[deferred]` intentionally postponed (rationale inline)

---

## A. Engine extraction (Phase A)

| Slice | Status | Notes |
|---|---|---|
| A.1–A.6: move db/models/commands/executor/scheduler/triggers/workflows/plugins to `crates/orbit-engine` | `[done]` | Workspace layout: `src-tauri`, `crates/orbit-engine`, `crates/orbit-server`. |
| A.7: decouple engine from `tauri::AppHandle` behind `RuntimeHost` trait | `[done]` | `RuntimeHost` now backs desktop/headless event emission; `orbit-server` boots executor + scheduler. Desktop-only agent/plugin paths still error cleanly without a Tauri host. |
| A.8: standalone `bin/orbit-server.rs` smoke (`BindMode::LoopbackToken`) | `[done]` | `cargo build -p orbit-server` clean. |

---

## B. Data layer — per-backend repo traits (Phase B)

### B.1 Trait surface (`crates/orbit-engine/src/db/repos/mod.rs`)

16 aggregates currently defined on `Repos`:

`agents`, `bus_messages`, `bus_subscriptions`, `chat`, `project_board_columns`, `project_boards`, `project_workflows`, `projects`, `runs`, `schedules`, `tasks`, `users`, `work_items`, `work_item_events`, `workflow_runs`, `workflow_seen_items`.

Status: `[done]` for the repo-backed command surface listed below. The original-plan goal of moving every executor/scheduler/plugin/workflow persistence path to native backend traits remains `[partial]`; `AppContext.db` and the SQLite compatibility facade still support local runtime internals.

### B.2 SqliteRepos impl (`crates/orbit-engine/src/db/repos/sqlite.rs`)

- `[done]` All read methods for the 15 aggregates above.
- `[done]` Helpers: `with_conn`, `with_conn_mut`, `IntoStringErr::err_str`.
- `[done]` Write paths migrated:
  - `AgentRepo::create_basic`, `set_model_config`, `update_basic`, `delete`, `next_available_id`
  - `ProjectRepo::create_basic`, `update`, `delete`, `add_agent`, `remove_agent`
  - `ProjectWorkflowRepo::list_enabled_triggers`
  - `RunRepo::cancel`
  - `RunRepo::update_state`
  - `RunRepo::create_scheduled_task_run`, `create_manual_run`, `create_retry_run`, `create_bus_run`, `recover_orphans`
  - `ScheduleRepo::create`, `toggle`, `delete`, due/recompute listing, schedule advancement helpers
  - `TaskRepo::create`, `update`, `delete`
  - `UserRepo::create`
  - `BusSubscriptionRepo::list_enabled_for_source`, `create`, `set_enabled`, `delete`
  - `ProjectBoardRepo::create`, `update`, `delete` (cross-table re-parenting)
  - `WorkflowRunRepo::cancel`
  - `WorkflowRunRepo::create_run`, `update_status`, `insert_step`, `finish_step`, `recover_orphans`
  - `WorkflowSeenItemRepo::filter_unseen`
  - `ChatRepo::create_session`, `rename_session`, `archive_session`, `unarchive_session`, `delete_session`, `append_message`, `upsert_active_skill`
  - `WorkItemRepo::create`, `update`, `delete`, `claim`, `move_item`, `reorder`, `block`, `unblock`, `complete`, comment CRUD
- `[next]` Coordinator-style writes that span aggregates or filesystem/cloud side effects can now migrate command signatures/adapters to `AppContext`. Keep the actual repo extraction scoped per command.

### B.3 Command migrations (`crates/orbit-engine/src/commands/*.rs`)

Per-file remaining direct `DbPool` command arguments are zero. Audit note: this table tracked command signatures, not all direct local SQLite usage. Several coordinator paths still call `AppContext.db` for local engine state while repo/native backend traits continue to grow.

| File | DbPool refs | Status | Notes / next slice |
|---|---|---|---|
| `workflow_runs.rs` | 0 | `[partial]` | Tauri + shim start paths use `AppContext`; run/step persistence now uses `WorkflowRunRepo`. Node execution still carries local runtime DB for node internals. |
| `terminals.rs` | 0 | `[done]` | Session agent lookup uses `ChatRepo::session_meta`; PTY lifecycle still uses Tauri registry. |
| `triggers.rs` | 0 | `[done]` | Commands use `AppContext`; subscription reconcile can reuse `PluginManager` without Tauri state extraction. |
| `llm.rs` | 0 | `[done]` | API-key sync and agent-loop trigger paths use `AppContext` cloud/db/executor coordinator. |
| `tasks.rs` | 0 | `[partial]` | CRUD uses `TaskRepo`; manual trigger path uses `AppContext` db/executor coordinator. |
| `projects.rs` | 0 | `[partial]` | Commands use `AppContext`; async project membership checks moved to executor-side `ProjectRepo` helper. Workspace/default-board coordinators still use local runtime DB state. |
| `agents.rs` | 0 | `[partial]` | Create/update/delete use `AppContext`; agent events emit through `RuntimeHost`; slug-rename/workspace coordinators remain local. |
| `pulse.rs` | 0 | `[partial]` | Pulse config read/update use `AppContext` db while keeping workspace + task/schedule/session coordinator logic. |
| `skills.rs` | 0 | `[partial]` | Skill list/delete cleanup paths use `AppContext` db; file-backed create/read unchanged. |
| `workspace.rs` | 0 | `[partial]` | Workspace config/prompt writes use `AppContext`; `agent:config_changed` emits through `RuntimeHost`; filesystem-backed workspace state is local-first. |
| `auth.rs` | 0 | `[partial]` | Auth commands/adapters use `AppContext` auth/cloud/db state directly; cloud sync remains optional accessory state. |
| `project_board_columns.rs` | 0 | `[partial]` | Revision-checked CRUD uses `AppContext` db/cloud; transaction helpers remain local. |
| `plugins.rs` | 0 | `[partial]` | Entity DB reads + reload/secret reconciliation use `AppContext`; install/OAuth lifecycle still requires Tauri host and local plugin state. |
| `project_workflows.rs` | 0 | `[done]` | Workflow CRUD/enable and tool read/scope paths use `ProjectWorkflowRepo`; cloud sync remains command/tool-side. |
| `chat.rs` | 0 | `[partial]` | Commands/adapters use `AppContext`; session CRUD/message append/skill activation/cancel polling use `ChatRepo`. Streaming internals still consume `AppContext.db` until executor/context APIs move to repos. |
| `work_items.rs` | 0 | `[done]` | Commands, HTTP shim, executor tool, and workflow board node use `WorkItemRepo`; legacy `*_with_db` helpers removed. |

### B.4 `tenant_id` column

- `[done]` Migration 31 adds `tenant_id TEXT NOT NULL DEFAULT 'local'` to all tables.
- `[done]` `RepoCtx { tenant_id }` added; `SqliteRepos::new` defaults to `'local'`, `with_tenant`/`with_ctx` allow explicit context, and repo-owned `sqlite.rs` inserts write `tenant_id` explicitly.
- `[done]` All `INSERT INTO` SQL blocks under `crates/orbit-engine/src` now write `tenant_id` explicitly, either from `RepoCtx`, a parent row (`projects`, `agents`, `tasks`, `chat_sessions`, etc.), or `'local'` for bootstrap/test scaffolding.
- `[done]` `SqliteRepos` read/update/delete paths now bind `RepoCtx.tenant_id` across tasks, agents, projects/memberships, schedules, users, runs, workflow runs, boards/columns, bus, chat, work items/comments/events, and project workflows.
- `[done]` Raw-query audit batch 1: workflow run store updates, workflow seen-items dedupe, default-board backfill, and project board/column transaction helpers now scope through parent `tenant_id`.
- `[done]` Raw-query audit batch 2: command coordinators, scheduler/triggers, workflow store, plugin entity/core helpers, cloud import/export, executor loops/context/compaction/session worktree, and executor tools now scope tenant-bearing reads/mutations through a parent row or explicit `'local'` legacy default.
- `[done]` B.4 verification sweep: insert audit is clean; raw SQL audit has only false positives from log strings, prompts, or dynamic SQL whose tenant predicate is appended in a separate string.

### B.5 PostgreSQL backend (Phase C — repo-backed surface)

- `[done]` `PgRepos` implements the existing repo trait surface against `sqlx::PgPool`; all queries bind `RepoCtx.tenant_id`. Lives at `crates/orbit-engine/src/db/repos/postgres.rs`.
- `[done]` RLS regression harness added at `crates/orbit-engine/tests/pg_repos_rls.rs`; run with `ORBIT_TEST_POSTGRES_URL=... cargo test -p orbit-engine --test pg_repos_rls -- --ignored`.
- `[done]` B.5 verification sweep: `cargo fmt --check`, `git diff --check`, `cargo test -p orbit-engine --test pg_repos_rls --no-run`, `cargo check --workspace`, and `cargo build -p orbit-server` pass with the same pre-existing warnings listed in prior B.4 verification.
- `[done]` Phase C schema/runtime slice: consolidated Postgres migration lives under `crates/orbit-engine/src/db/migrations/postgres/`, creates the current tenant-bearing schema, forces RLS on every table, and grants the default-deny `application_role`.
- `[done]` `orbit_server` selects `PgRepos` only when `ORBIT_DB_BACKEND=postgres`, uses `ORBIT_POSTGRES_URL` (or `DATABASE_URL` fallback after explicit opt-in), requires `ORBIT_TENANT_ID`, requires `ORBIT_POSTGRES_MIGRATIONS_URL` when `ORBIT_APPLY_POSTGRES_MIGRATIONS=1`, and wires the selected repo backend into `AppContext`, executor, and scheduler.
- `[done]` Online migration story, PgBouncer/session-pool requirement, migration test harness command, and EXPLAIN baseline list are documented in `docs/ops/postgres-phase-c.md`.
- `[done]` Phase C completion boundary is explicit: Postgres covers repo-backed command surfaces, workflow run persistence, seen-items dedupe, scheduler state helpers, manual/scheduled run starts, executor run state/retry/bus transitions, trigger subscription reconciliation, trigger dispatch matching, and trigger-to-workflow execution handoff. Plugin execution internals, trigger-to-agent execution handoff, remaining workflow node internals, schedule-tool helper internals, workspace internals, and chat/session loop internals are intentionally local-runtime sidecars backed by SQLite until a later repo/native-backend slice migrates them.

### B.6 Sqlx swap on the SQLite path

- `[done]` Removed external `rusqlite`, `r2d2`, and `r2d2_sqlite` dependencies from the engine/desktop crates; `sqlx` now enables the SQLite driver.
- `[done]` `DbPool` now resolves to an SQLx-backed compatibility pool, so `SqliteRepos` and remaining direct `DbPool` callers run on `sqlx::SqlitePool` without linking a second SQLite driver.
- `[done]` Added `sqlite_compat` facade for the legacy sync-style query surface while follow-on slices migrate the remaining direct `DbPool` call sites to native repo methods.
- `[done]` B.6 verification sweep: `cargo fmt --check`, `git diff --check`, `cargo check --workspace`, `cargo build -p orbit-server`, `cargo test -p orbit-engine --tests --no-run`, `cargo test -p orbit-engine workflows:: -- --nocapture`, and `cargo test -p orbit-engine active_skills_round_trip_through_db_state -- --nocapture` pass.

### Original-plan audit blockers / remaining verification

- `[done]` Workflow run persistence sub-slice: orchestrator start/status/step writes now flow through `WorkflowRunRepo` for SQLite/Postgres while preserving orchestrator-owned event emission.
- `[done]` Workflow seen-items sub-slice: feed/http dedupe now flows through `WorkflowSeenItemRepo` for SQLite/Postgres while the pure fingerprinting helper stays in workflow runtime code.
- `[done]` Scheduler state sub-slice: due schedule polling, schedule advancement/recompute, scheduled task run creation, run orphan recovery, and workflow-run orphan recovery now flow through repo traits. Headless cloud scheduler activation still waits on the executor repo path so task execution state does not split across backends.
- `[done]` Trigger subscription reconciler sub-slice: desired subscription computation now gets agent rows and enabled workflow triggers through repo traits. Agent `listen_bindings` remain local workspace config by design.
- `[done]` Trigger dispatch matching sub-slice: inbound `trigger.emit` dispatch now awaits repo-backed agent/workflow matching before emitting workflow events or spawning desktop agent runs. Workflow execution handoff remains event-only.
- `[done]` Trigger-to-workflow execution sub-slice: matched inbound workflow triggers now start `WorkflowOrchestrator` runs with the normalized trigger payload while preserving the existing `trigger:workflow` event signal.
- `[done]` Executor run-state sub-slice: executor running/success/failure/timed-out/cancelled/skipped transitions now persist through `RunRepo::update_state`/`cancel` while retry and bus-triggered run creation moved in follow-on slices.
- `[done]` Executor retry sub-slice: retry eligibility reads the prior run through `RunRepo::get`, and retry run rows are created through `RunRepo::create_retry_run`.
- `[done]` Executor bus orchestration sub-slice: terminal run lookup, enabled subscription lookup, target task lookup, bus message creation, and bus-triggered run creation now flow through `RunRepo`, `TaskRepo`, and `BusSubscriptionRepo`.
- `[done]` Manual run-start sub-slice: task command trigger, agent-loop command trigger, and pulse tool run trigger now create pending manual runs through `RunRepo::create_manual_run` before enqueueing executor work.
- `[done]` Full Postgres engine path boundary: selected repo backend now reaches `AppContext`, executor, scheduler, triggers, and workflow orchestration; remaining direct SQLite internals are documented as local-runtime sidecars, not shared Postgres state.
- `[next]` Live RLS integration: run `ORBIT_TEST_POSTGRES_URL=... ORBIT_TEST_POSTGRES_APPLY_MIGRATIONS=1 cargo test -p orbit-engine --test pg_repos_rls -- --ignored --nocapture`.
- `[next]` Local desktop smoke: verify Tauri desktop still boots, lists Projects, starts a chat/session, and can create/update local agents/projects/work items.
- `[next]` Headless local smoke: boot `orbit-server` with only `ORBIT_DATA_DIR`, verify it uses SQLite, loads the UI, and lists Projects.
- `[next]` Query baselines: capture `EXPLAIN ANALYZE` for the documented top Postgres queries with RLS enabled.
- `[next]` Performance parity: measure the five hottest local SQLite query paths against the pre-sqlx baseline or document accepted deltas.

---

## How to claim a slice

1. Edit this file: change the slice's status to `[wip]` and add your agent name + branch.
2. Work in a fresh branch off `feat/extract-engine`.
3. When merged, flip to `[done]` and update the per-file count if relevant.
4. If you discover a new sub-slice, add a row.

---

## Critical files (orientation)

- `crates/orbit-engine/src/db/repos/mod.rs` — trait definitions; grow it as you migrate writes.
- `crates/orbit-engine/src/db/repos/sqlite.rs` — concrete impls; uses `with_conn`/`with_conn_mut` helpers.
- `crates/orbit-engine/src/app_context.rs` — `AppContext` holds the pool + `Arc<dyn Repos>`. Commands take `app: tauri::State<AppContext>` and call `app.repos().<aggregate>().<method>()`.
- `crates/orbit-engine/src/commands/mod.rs` — Tauri command registry; matches the migration state above.
- `crates/orbit-engine/src/models/*` — DTOs returned by trait methods. Add typed return shapes here, not in `commands/`.

## Verification per slice

- `cargo check --workspace` clean
- `cargo build -p orbit-server` clean
- `cargo test --workspace` not yet wired for repo regression — write the RLS cross-tenant test as part of B.5.

---

## D. Control plane / cloud auth foundation (Phase D)

- `[done]` Shim JWT auth foundation: `ORBIT_SHIM_AUTH_MODE=jwt` enables HS256 bearer-token verification for HTTP and WebSocket shim traffic; tokens require `sub`, `tenant_id`, and `exp`, with optional issuer/audience validation and `ORBIT_TENANT_ID` tenant-match enforcement.
- `[next]` Request-scoped tenant context: carry verified JWT claims through shim request handling and move Postgres tenant binding from per-pool `SET app.tenant_id` to transaction-local `SET LOCAL app.tenant_id`.
- `[next]` Control-plane v0 service: issue short-lived access JWTs, expose tenant discovery, and add the first provision/resume endpoints.
