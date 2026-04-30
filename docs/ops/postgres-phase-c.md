# Postgres Phase C Runbook

Phase C adds an optional shared-runtime Postgres backend for repo-backed command
paths and the background executor/scheduler state transitions that already flow
through the repo facade. Local mode remains the default and primary runtime:
desktop and the headless server use SQLite unless Postgres is explicitly
selected.

## Runtime Modes

SQLite/local mode is the default:

```sh
cargo run -p orbit-server
```

Ambient `DATABASE_URL` values do not change the backend. Postgres backs the repo
facade only when the Orbit backend switch is set:

```sh
export ORBIT_DB_BACKEND=postgres
export ORBIT_POSTGRES_URL=postgres://orbit_app:...@localhost:5432/orbit
export ORBIT_TENANT_ID=tenant_dev
cargo run -p orbit-server
```

Apply the schema/RLS bootstrap explicitly:

```sh
export ORBIT_DB_BACKEND=postgres
export ORBIT_POSTGRES_URL=postgres://orbit_app:...@localhost:5432/orbit
export ORBIT_TENANT_ID=tenant_dev
export ORBIT_POSTGRES_MIGRATIONS_URL=postgres://orbit_owner:...@localhost:5432/orbit
ORBIT_APPLY_POSTGRES_MIGRATIONS=1 cargo run -p orbit-server
```

`ORBIT_POSTGRES_MIGRATIONS_URL` is required when migration application is
enabled. Use a schema-owner URL for migration application. Use an application
user that inherits `application_role` for traffic:

```sql
CREATE USER orbit_app WITH PASSWORD '...';
GRANT application_role TO orbit_app;
```

Each pool connection sets `app.tenant_id` at connect time. This matches the
current per-tenant repo pool model. Phase D JWT work should move request-scoped
traffic to transaction-local `SET LOCAL app.tenant_id = <jwt tenant>` once the
shim carries an authenticated session into every repo call.

## Shim Authentication

`orbit-server` defaults to loopback dev-token auth:

```sh
cargo run -p orbit-server
```

Cloud/self-host deployments can opt into JWT auth:

```sh
export ORBIT_SHIM_AUTH_MODE=jwt
export ORBIT_JWT_HS256_SECRET=...
export ORBIT_JWT_ISSUER=orbit-control        # optional
export ORBIT_JWT_AUDIENCE=orbit-engine       # optional
export ORBIT_TENANT_ID=tenant_dev            # required for Postgres; optional tenant check for SQLite
cargo run -p orbit-server
```

JWT claims must include `sub`, `tenant_id`, and `exp`. When
`ORBIT_TENANT_ID` is set, the shim rejects tokens whose `tenant_id` does not
match the configured tenant. This is a Phase D foundation: HS256 is enough for
local control-plane integration and self-host smoke tests; production SaaS
should move to JWKS-backed asymmetric verification before public launch.

## Phase C Boundary

When `ORBIT_DB_BACKEND=postgres`, `orbit-server` wires the selected `PgRepos`
instance into `AppContext`, the executor, the scheduler, trigger matching, and
workflow orchestration. That means repo-backed command surfaces, workflow run
persistence, schedule polling/advancement, run state transitions, retry/bus
run creation, trigger subscription reconciliation, trigger dispatch matching,
and trigger-to-workflow execution use Postgres under the configured tenant.

The remaining direct `DbPool` callers are an explicit local-runtime sidecar:
workspace files/config, installed plugin metadata/runtime internals, chat and
session loop internals, schedule-tool helper internals, and workflow node
internals that operate on local process state. Those paths continue to use the
SQLite database under `ORBIT_DATA_DIR`, even when the repo backend is Postgres.
Do not treat that sidecar as shared multi-tenant data; migrate a path behind a
repo/native backend trait before depending on it for cross-device or shared
runtime behavior.

## Local Mode Contract

- Desktop and default `orbit-server` mode use SQLite under the Orbit data dir.
- Local workspaces, installed plugins, logs, dev tokens, and agent files remain
  under that data dir.
- Cloud sync is optional and no-ops when no cloud client is configured.
- Postgres is an accessory deployment backend and never runs unless
  `ORBIT_DB_BACKEND=postgres` is set.
- In Postgres mode, shared engine state is limited to paths that use the repo
  facade. Local-runtime sidecar paths continue to use SQLite and are not part
  of the shared Postgres contract.

## RLS Contract

The bootstrap schema creates tenant-bearing tables with:

- `tenant_id TEXT NOT NULL`
- `ALTER TABLE ... ENABLE ROW LEVEL SECURITY`
- `ALTER TABLE ... FORCE ROW LEVEL SECURITY`
- a single `tenant_isolation` policy for `application_role`

Without `app.tenant_id`, `application_role` reads zero rows and cannot write.
Superusers and roles with `BYPASSRLS` must not be used for application traffic.

PgBouncer must run in session-pool mode for the current per-connection tenant
binding. Transaction pooling is only acceptable after repo calls use
transaction-local `SET LOCAL`.

## Online Migration Rules

For shared Postgres DDL, use expand-and-contract:

1. Expand with nullable columns, defaults that do not rewrite large tables, new
   tables, and `CREATE INDEX CONCURRENTLY`.
2. Deploy code that writes both old and new shapes or tolerates either shape.
3. Backfill in bounded tenant batches ordered by primary key.
4. Add constraints as `NOT VALID`, then `VALIDATE CONSTRAINT`.
5. Contract only after old code is drained.

Avoid long table locks in request-serving windows. Never add a required column
with a volatile default to a hot table in one step.

## Migration Test Harness

Run the ignored Postgres regression against an isolated database:

```sh
export ORBIT_TEST_POSTGRES_URL=postgres://orbit_owner:...@localhost:5432/orbit_test
ORBIT_TEST_POSTGRES_APPLY_MIGRATIONS=1 \
  cargo test -p orbit-engine --test pg_repos_rls -- --ignored --nocapture
```

The test creates two tenant pools, writes through the repo surface for both
tenants, and verifies cross-tenant reads return nothing.

## Query Baselines

Capture `EXPLAIN (ANALYZE, BUFFERS, VERBOSE)` with RLS enabled for:

- task list and task detail
- agent list and agent detail
- project list with membership count
- project board column list
- work item board list
- chat session list
- chat message page
- workflow run list
- schedule due scan
- bus thread/message list

Store before/after plans with the migration PR. Any hot query regression over
10% needs either an index adjustment or an explicit acceptance note.
