# Postgres Phase C Runbook

Phase C adds a shared-runtime Postgres backend for repo-backed command paths.
The current headless server still keeps a local SQLite pool for executor,
scheduler, plugin, and workspace internals that have not moved fully behind
repo traits.

## Runtime Modes

SQLite remains the default:

```sh
cargo run -p orbit-server
```

Postgres backs the repo facade when `DATABASE_URL` is a Postgres URL:

```sh
export DATABASE_URL=postgres://orbit_owner:...@localhost:5432/orbit
export ORBIT_TENANT_ID=tenant_dev
cargo run -p orbit-server
```

Apply the schema/RLS bootstrap explicitly:

```sh
export ORBIT_POSTGRES_MIGRATIONS_URL=postgres://orbit_owner:...@localhost:5432/orbit
ORBIT_APPLY_POSTGRES_MIGRATIONS=1 cargo run -p orbit-server
```

Use a schema-owner URL for migration application. Use an application user that
inherits `application_role` for traffic:

```sql
CREATE USER orbit_app WITH PASSWORD '...';
GRANT application_role TO orbit_app;
```

Each pool connection sets `app.tenant_id` at connect time. This matches the
current per-tenant repo pool model. Phase D JWT work should move request-scoped
traffic to transaction-local `SET LOCAL app.tenant_id = <jwt tenant>` once the
shim carries an authenticated session into every repo call.

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
