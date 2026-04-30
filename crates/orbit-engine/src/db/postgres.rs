//! Postgres bootstrap helpers for the shared-runtime backend.
//!
//! The repository layer is intentionally tenant-scoped at the SQL level, but
//! shared Postgres also needs RLS to be active before application traffic uses
//! the pool. These helpers keep that wiring in one place for tests and the
//! standalone server.

use sqlx::postgres::{PgPool, PgPoolOptions};

pub const POSTGRES_SCHEMA: &str = include_str!("migrations/postgres/0001_schema.sql");

fn db_err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Apply the consolidated Postgres schema and RLS policy set.
///
/// Run this with a schema-owner URL, not the restricted application role. The
/// application role gets only DML grants and is expected to inherit the
/// `application_role` policies declared by the migration.
pub async fn apply_migrations(pool: &PgPool) -> Result<(), String> {
    sqlx::raw_sql(POSTGRES_SCHEMA)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(db_err)
}

pub async fn owner_pool(database_url: &str) -> Result<PgPool, String> {
    PgPoolOptions::new()
        .max_connections(2)
        .connect(database_url)
        .await
        .map_err(db_err)
}

/// Build a Postgres pool whose connections are tenant-bound.
///
/// For the current repo API the tenant is fixed per pool. Phase D request JWT
/// work can switch this to transaction-local `SET LOCAL` after the shim carries
/// an authenticated request context into each repo call.
pub async fn tenant_pool(
    database_url: &str,
    tenant_id: impl Into<String>,
) -> Result<PgPool, String> {
    let tenant_id = tenant_id.into();
    PgPoolOptions::new()
        .max_connections(10)
        .after_connect(move |conn, _meta| {
            let tenant_id = tenant_id.clone();
            Box::pin(async move {
                sqlx::query("SELECT set_config('app.tenant_id', $1, false)")
                    .bind(&tenant_id)
                    .execute(conn)
                    .await?;
                Ok(())
            })
        })
        .connect(database_url)
        .await
        .map_err(db_err)
}
