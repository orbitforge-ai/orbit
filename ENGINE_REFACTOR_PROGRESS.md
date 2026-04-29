# Phase Progress — Engine extraction + per-backend repo traits

Source plan: `/Users/matwaroff/.claude/plans/quirky-jingling-candy.md`
Council edits: `/Users/matwaroff/.claude/plans/please-review-this-plan-elegant-meteor.md`

This is a living index of what's done in Phase B so work can be split across agents without overlap. Update it as you finish slices. Keep entries terse — one line each.

---

## Status legend

- `[done]` merged on this branch (`feat/extract-engine`)
- `[wip]` someone is actively working on it (claim by editing this file)
- `[next]` ready to pick up; preconditions met
- `[blocked]` waiting on another slice; note which one
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
- `[deferred]` Coordinator-style writes that span aggregates inside one tx: `pulse`, `llm`, `auth`, `workspace` commands. Keep on `DbPool` until executor decoupling lands (A.7).

### B.3 Command migrations (`crates/orbit-engine/src/commands/*.rs`)

Per-file remaining `DbPool` references (lower = closer to fully migrated). Read-path migrations for these are done; remaining counts are write paths still on the legacy `DbPool` path.

| File | DbPool refs | Status | Notes / next slice |
|---|---|---|---|
| `workflow_runs.rs` | 3 | `[next]` | Smallest surface; finish first. |
| `terminals.rs` | 4 | `[next]` | PTY lifecycle, light DB use. |
| `triggers.rs` | 4 | `[next]` | Cross-aggregate; coordinate with `triggers/dispatcher.rs`. |
| `llm.rs` | 4 | `[blocked]` | Coordinator command — defer until A.7. |
| `tasks.rs` | 5 | `[next]` | Mostly write paths; trait methods exist. |
| `projects.rs` | 5 | `[next]` | `create_project` retains workspace-init side effects. |
| `agents.rs` | 6 | `[next]` | `create_agent` / `update_agent` slug-rename → propose `AgentRepo::rename_with_references`. |
| `pulse.rs` | 6 | `[blocked]` | Coordinator — defer. |
| `skills.rs` | 6 | `[next]` | Simple write paths. |
| `workspace.rs` | 6 | `[blocked]` | Coordinator — defer. |
| `auth.rs` | 7 | `[blocked]` | Coordinator — defer. |
| `project_board_columns.rs` | 10 | `[next]` | Revision-checked CRUD; design `ProjectBoardColumnRepo` write methods first. |
| `plugins.rs` | 14 | `[next]` | Add `PluginRepo` trait surface (not yet defined). |
| `project_workflows.rs` | 18 | `[next]` | Graph normalization + transactional swap; design carefully. |
| `chat.rs` | 28 | `[blocked]` | Streaming executor + worktree lifecycle entanglement; defer until executor decoupling. |
| `work_items.rs` | 36 | `[blocked]` | Called from agent tools as `*_with_db`; defer until executor decoupling. |

### B.4 `tenant_id` column

- `[done]` Migration 31 adds `tenant_id TEXT NOT NULL DEFAULT 'local'` to all tables.
- `[next]` Plumb tenant_id into trait methods (currently writes default to `'local'`). Touches every `INSERT` in `sqlite.rs` + adds a tenant context param to `Repos`. Design as `RepoCtx { tenant_id }` threaded through the facade.

### B.5 PostgreSQL backend (Phase C — unblocked)

- `[next]` Implement `PgRepos` against the existing trait surface using `sqlx::PgPool`. Parallel-safe with B.3 work. Lives at `crates/orbit-engine/src/db/repos/postgres.rs`.
- `[next]` RLS regression test: run every command twice with two tenants; assert no cross-leak.
- `[blocked]` Online-migration story for shared multi-tenant Postgres. Comes after PgRepos boots.

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
