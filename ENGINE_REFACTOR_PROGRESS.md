# Phase Progress — Engine extraction + per-backend repo traits

Source plan: `/Users/matwaroff/.claude/plans/quirky-jingling-candy.md`
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

15 aggregates currently defined on `Repos`:

`agents`, `bus_messages`, `bus_subscriptions`, `chat`, `project_board_columns`, `project_boards`, `project_workflows`, `projects`, `runs`, `schedules`, `tasks`, `users`, `work_items`, `work_item_events`, `workflow_runs`.

Status: `[done]` for read paths and the write paths listed below. The trait itself can grow — add methods as you migrate write paths.

### B.2 SqliteRepos impl (`crates/orbit-engine/src/db/repos/sqlite.rs`)

- `[done]` All read methods for the 15 aggregates above.
- `[done]` Helpers: `with_conn`, `with_conn_mut`, `IntoStringErr::err_str`.
- `[done]` Write paths migrated:
  - `AgentRepo::create_basic`, `set_model_config`, `update_basic`, `delete`, `next_available_id`
  - `ProjectRepo::create_basic`, `update`, `delete`, `add_agent`, `remove_agent`
  - `RunRepo::cancel`
  - `ScheduleRepo::create`, `toggle`, `delete`
  - `TaskRepo::create`, `update`, `delete`
  - `UserRepo::create`
  - `BusSubscriptionRepo::create`, `set_enabled`, `delete`
  - `ProjectBoardRepo::create`, `update`, `delete` (cross-table re-parenting)
  - `WorkflowRunRepo::cancel`
  - `ChatRepo::create_session`, `rename_session`, `archive_session`, `unarchive_session`, `delete_session`, `append_message`, `upsert_active_skill`
  - `WorkItemRepo::create`, `update`, `delete`, `claim`, `move_item`, `reorder`, `block`, `unblock`, `complete`, comment CRUD
- `[next]` Coordinator-style writes that span aggregates or filesystem/cloud side effects can now migrate command signatures/adapters to `AppContext`. Keep the actual repo extraction scoped per command.

### B.3 Command migrations (`crates/orbit-engine/src/commands/*.rs`)

Per-file remaining `DbPool` references (lower = closer to fully migrated). Read-path migrations for these are done; remaining counts are write paths still on the legacy `DbPool` path.

| File | DbPool refs | Status | Notes / next slice |
|---|---|---|---|
| `workflow_runs.rs` | 0 | `[done]` | Tauri + shim start paths use `AppContext` runtime/db; read/cancel paths use repo. |
| `terminals.rs` | 0 | `[done]` | Session agent lookup uses `ChatRepo::session_meta`; PTY lifecycle still uses Tauri registry. |
| `triggers.rs` | 0 | `[done]` | Commands use `AppContext`; subscription reconcile can reuse `PluginManager` without Tauri state extraction. |
| `llm.rs` | 0 | `[done]` | API-key sync and agent-loop trigger paths use `AppContext` cloud/db/executor coordinator. |
| `tasks.rs` | 0 | `[done]` | CRUD uses `TaskRepo`; manual trigger path uses `AppContext` db/executor coordinator. |
| `projects.rs` | 0 | `[done]` | Commands use `AppContext`; async project membership checks moved to executor-side `ProjectRepo` helper. |
| `agents.rs` | 0 | `[done]` | Create/update/delete use `AppContext`; agent events emit through `RuntimeHost`; slug-rename coordinator remains local. |
| `pulse.rs` | 0 | `[done]` | Pulse config read/update use `AppContext` db while keeping workspace + task/schedule/session coordinator logic. |
| `skills.rs` | 0 | `[done]` | Skill list/delete cleanup paths use `AppContext` db; file-backed create/read unchanged. |
| `workspace.rs` | 0 | `[done]` | Workspace config/prompt writes use `AppContext`; `agent:config_changed` emits through `RuntimeHost`. |
| `auth.rs` | 0 | `[done]` | Auth commands/adapters use `AppContext` auth/cloud/db state directly. |
| `project_board_columns.rs` | 0 | `[done]` | Revision-checked CRUD uses `AppContext` db/cloud; transaction helpers remain local. |
| `plugins.rs` | 0 | `[done]` | Entity DB reads + reload/secret reconciliation use `AppContext`; install/OAuth lifecycle still requires Tauri host. |
| `project_workflows.rs` | 0 | `[done]` | Workflow CRUD/enable and tool read/scope paths use `ProjectWorkflowRepo`; cloud sync remains command/tool-side. |
| `chat.rs` | 0 | `[done]` | Commands/adapters use `AppContext`; session CRUD/message append/skill activation/cancel polling use `ChatRepo`. Streaming internals still consume `AppContext.db` until executor/context APIs move to repos. |
| `work_items.rs` | 0 | `[done]` | Commands, HTTP shim, executor tool, and workflow board node use `WorkItemRepo`; legacy `*_with_db` helpers removed. |

### B.4 `tenant_id` column

- `[done]` Migration 31 adds `tenant_id TEXT NOT NULL DEFAULT 'local'` to all tables.
- `[done]` `RepoCtx { tenant_id }` added; `SqliteRepos::new` defaults to `'local'`, `with_tenant`/`with_ctx` allow explicit context, and repo-owned `sqlite.rs` inserts write `tenant_id` explicitly.
- `[done]` All `INSERT INTO` SQL blocks under `crates/orbit-engine/src` now write `tenant_id` explicitly, either from `RepoCtx`, a parent row (`projects`, `agents`, `tasks`, `chat_sessions`, etc.), or `'local'` for bootstrap/test scaffolding.
- `[done]` `SqliteRepos` read/update/delete paths now bind `RepoCtx.tenant_id` across tasks, agents, projects/memberships, schedules, users, runs, workflow runs, boards/columns, bus, chat, work items/comments/events, and project workflows.
- `[done]` Raw-query audit batch 1: workflow run store updates, workflow seen-items dedupe, default-board backfill, and project board/column transaction helpers now scope through parent `tenant_id`.
- `[done]` Raw-query audit batch 2: command coordinators, scheduler/triggers, workflow store, plugin entity/core helpers, cloud import/export, executor loops/context/compaction/session worktree, and executor tools now scope tenant-bearing reads/mutations through a parent row or explicit `'local'` legacy default.
- `[done]` B.4 verification sweep: insert audit is clean; raw SQL audit has only false positives from log strings, prompts, or dynamic SQL whose tenant predicate is appended in a separate string.

### B.5 PostgreSQL backend (Phase C — unblocked)

- `[done]` `PgRepos` implements the existing repo trait surface against `sqlx::PgPool`; all queries bind `RepoCtx.tenant_id`. Lives at `crates/orbit-engine/src/db/repos/postgres.rs`.
- `[done]` RLS regression harness added at `crates/orbit-engine/tests/pg_repos_rls.rs`; run with `ORBIT_TEST_POSTGRES_URL=... cargo test -p orbit-engine --test pg_repos_rls -- --ignored`.
- `[done]` B.5 verification sweep: `cargo fmt --check`, `git diff --check`, `cargo test -p orbit-engine --test pg_repos_rls --no-run`, `cargo check --workspace`, and `cargo build -p orbit-server` pass with the same pre-existing warnings listed in prior B.4 verification.

Follow-on after B.5:

- `[next]` Online-migration story for shared multi-tenant Postgres now that `PgRepos` boots.

### B.6 Sqlx swap on the SQLite path

- `[deferred]` Drop rusqlite/r2d2; rewrite `SqliteRepos` on `sqlx::SqlitePool`. Multi-day rewrite blocked by ~40 files outside `commands/` that hold `DbPool` directly (executor, scheduler, triggers, workflows, plugins, app_context). Not blocking Phase C.

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
