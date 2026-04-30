//! Standalone Orbit engine server.
//!
//! Boots the same `orbit-engine` crate that the Tauri desktop app uses, but
//! without a Tauri runtime. Reads config from environment variables, opens a
//! local SQLite data layer by default, constructs an `AppContext` with
//! `tauri: None`, and exposes everything over the existing HTTP+WS shim.
//! An explicitly selected Postgres repo backend is available for shared
//! runtime/cloud deployments.
//!
//! Status: **partial**. The shim and DB-only command paths (e.g.
//! `list_projects`) work end-to-end. Command paths that still go through
//! `ctx.app()?.state::<T>()` will return an error in headless mode until
//! their adapters are migrated to use `AppContext` fields directly (Phase
//! A.7 sweep).

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use orbit_engine::app_context::AppContext;
use orbit_engine::auth::{AuthMode, AuthState};
use orbit_engine::commands::users::ActiveUser;
use orbit_engine::db::cloud::CloudClientState;
use orbit_engine::db::connection::init as init_db;
use orbit_engine::db::postgres::{
    apply_migrations as apply_postgres_migrations, owner_pool, tenant_pool,
};
use orbit_engine::db::repos::{postgres::PgRepos, sqlite::SqliteRepos, Repos};
use orbit_engine::executor::bg_processes::BgProcessRegistry;
use orbit_engine::executor::engine::{
    AgentSemaphores, ExecutorEngine, ExecutorTx, SessionExecutionRegistry, UserQuestionRegistry,
};
use orbit_engine::executor::mcp_server;
use orbit_engine::executor::permissions::PermissionRegistry;
use orbit_engine::plugins::PluginManager;
use orbit_engine::runtime_host::headless_host;
use orbit_engine::scheduler::SchedulerEngine;
use orbit_engine::shim;
use tokio::sync::mpsc;

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,orbit_engine=info,orbit_server=info"));
    fmt().with_env_filter(filter).with_target(false).init();
}

fn data_dir_from_env() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("ORBIT_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    if let Ok(dir) = std::env::var("WORKSPACE_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let home =
        std::env::var("HOME").context("HOME, WORKSPACE_DIR, or ORBIT_DATA_DIR must be set")?;
    Ok(PathBuf::from(home).join(".orbit"))
}

fn bind_port_from_env() -> u16 {
    std::env::var("BIND_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(8765)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DbBackendConfig {
    Sqlite,
    Postgres {
        database_url: String,
        tenant_id: String,
        migrations_url: Option<String>,
    },
}

fn is_truthy(value: Option<&str>) -> bool {
    matches!(value, Some("1" | "true" | "yes"))
}

fn non_empty_env(get: &impl Fn(&str) -> Option<String>, key: &str) -> Option<String> {
    get(key).and_then(|value| {
        let value = value.trim().to_string();
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    })
}

fn require_postgres_url(value: Option<String>, key: &str) -> Result<String> {
    let url =
        value.ok_or_else(|| anyhow::anyhow!("{key} is required when ORBIT_DB_BACKEND=postgres"))?;
    if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        Ok(url)
    } else {
        Err(anyhow::anyhow!(
            "{key} must be a postgres:// or postgresql:// URL"
        ))
    }
}

fn db_backend_from_env() -> Result<DbBackendConfig> {
    db_backend_from(|key| std::env::var(key).ok())
}

fn db_backend_from(get: impl Fn(&str) -> Option<String>) -> Result<DbBackendConfig> {
    let backend = non_empty_env(&get, "ORBIT_DB_BACKEND")
        .unwrap_or_else(|| "sqlite".to_string())
        .to_ascii_lowercase();
    match backend.as_str() {
        "sqlite" | "local" => Ok(DbBackendConfig::Sqlite),
        "postgres" => {
            let database_url = require_postgres_url(
                non_empty_env(&get, "ORBIT_POSTGRES_URL")
                    .or_else(|| non_empty_env(&get, "DATABASE_URL")),
                "ORBIT_POSTGRES_URL or DATABASE_URL",
            )?;
            let tenant_id = non_empty_env(&get, "ORBIT_TENANT_ID").ok_or_else(|| {
                anyhow::anyhow!("ORBIT_TENANT_ID is required when ORBIT_DB_BACKEND=postgres")
            })?;
            let migrations_url =
                if is_truthy(non_empty_env(&get, "ORBIT_APPLY_POSTGRES_MIGRATIONS").as_deref()) {
                    Some(require_postgres_url(
                        non_empty_env(&get, "ORBIT_POSTGRES_MIGRATIONS_URL"),
                        "ORBIT_POSTGRES_MIGRATIONS_URL",
                    )?)
                } else {
                    None
                };
            Ok(DbBackendConfig::Postgres {
                database_url,
                tenant_id,
                migrations_url,
            })
        }
        other => Err(anyhow::anyhow!(
            "unsupported ORBIT_DB_BACKEND '{other}'; expected 'sqlite' or 'postgres'"
        )),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let version = env!("CARGO_PKG_VERSION");
    let data_dir = data_dir_from_env()?;
    let port = bind_port_from_env();

    tracing::info!(version, data_dir = %data_dir.display(), port, "orbit-server starting");

    // ── DB ──────────────────────────────────────────────────────────────────
    let db_pool = init_db(data_dir.clone()).map_err(|e| anyhow::anyhow!("init_db failed: {e}"))?;

    // ── Stub managed-state values ──────────────────────────────────────────
    // The headless server has no Tauri runtime, so command adapters that read
    // state via `ctx.app()?.state::<T>()` will fail. Adapters migrated to use
    // `AppContext` fields directly use these stubs.
    let auth_state = AuthState::new(AuthMode::Unset);
    let cloud_state = CloudClientState::empty();
    let active_user = ActiveUser::new("default_user".to_string());

    let (executor_tx_inner, executor_rx) =
        mpsc::unbounded_channel::<orbit_engine::executor::engine::RunRequest>();
    let executor_tx = ExecutorTx(executor_tx_inner);
    let runtime_host = headless_host();

    let agent_semaphores = AgentSemaphores::new();
    let session_registry = SessionExecutionRegistry::new();
    let permission_registry = PermissionRegistry::new();
    let user_question_registry = UserQuestionRegistry::new();
    let bg_process_registry = BgProcessRegistry::new();

    // Embedded MCP bridge — needs no AppHandle.
    let mcp_handle = mcp_server::start()
        .await
        .map_err(|e| anyhow::anyhow!("mcp bridge start failed: {e}"))?;

    let plugin_manager = Arc::new(PluginManager::init(db_pool.clone()));

    // Repository facade. Local SQLite is the default engine state; Postgres is
    // an explicit shared-runtime/cloud option for the repo-backed command surface.
    let repos: Arc<dyn Repos> = match db_backend_from_env()? {
        DbBackendConfig::Sqlite => {
            tracing::info!("using local SQLite repository backend");
            Arc::new(SqliteRepos::new(db_pool.clone()))
        }
        DbBackendConfig::Postgres {
            database_url,
            tenant_id,
            migrations_url,
        } => {
            tracing::info!(tenant_id, "ORBIT_DB_BACKEND=postgres; using PgRepos");
            let pg_pool = tenant_pool(&database_url, tenant_id.clone())
                .await
                .map_err(|e| {
                    anyhow::anyhow!("failed to initialize tenant-bound Postgres pool: {e}")
                })?;
            if let Some(migration_url) = migrations_url {
                tracing::info!("applying Postgres schema/RLS migrations");
                let migration_pool = owner_pool(&migration_url).await.map_err(|e| {
                    anyhow::anyhow!("failed to initialize Postgres migration pool: {e}")
                })?;
                apply_postgres_migrations(&migration_pool)
                    .await
                    .map_err(|e| anyhow::anyhow!("failed to apply Postgres migrations: {e}"))?;
            }
            Arc::new(PgRepos::with_tenant(pg_pool, tenant_id))
        }
    };

    // ── AppContext ──────────────────────────────────────────────────────────
    let ctx = AppContext::new(
        db_pool,
        repos.clone(),
        auth_state,
        cloud_state,
        active_user,
        executor_tx,
        agent_semaphores,
        session_registry,
        permission_registry,
        user_question_registry,
        bg_process_registry,
        mcp_handle,
        plugin_manager,
        None, // memory: Option<MemoryServiceState>
        None, // tauri: Option<AppHandle>
    );
    let ctx = Arc::new(ctx);

    // ── Shim ────────────────────────────────────────────────────────────────
    let dev_token_path = data_dir.join("dev_token");
    let mode = shim::auth::BindMode::loopback_with_file(dev_token_path)
        .map_err(|e| anyhow::anyhow!("shim token init failed: {e}"))?;
    let registry = shim::registry::build();

    let addr = shim::start(ctx.clone(), registry, mode, port)
        .await
        .map_err(|e| anyhow::anyhow!("shim failed to bind: {e}"))?;

    tracing::info!(%addr, "orbit-server bound");

    let log_dir = data_dir.join("logs");
    let engine = ExecutorEngine::new_with_repos(
        ctx.db.clone(),
        repos.clone(),
        executor_rx,
        ctx.executor_tx.0.clone(),
        runtime_host.clone(),
        ctx.agent_semaphores.clone(),
        ctx.sessions.clone(),
        ctx.permissions.clone(),
        log_dir.clone(),
        None,
        None,
    );
    tokio::spawn(async move { engine.run().await });

    let scheduler = SchedulerEngine::new_with_repos(
        ctx.db.clone(),
        repos,
        ctx.executor_tx.clone(),
        runtime_host.clone(),
        log_dir,
    );
    tokio::spawn(async move { scheduler.run().await });

    tracing::info!(
        "headless executor + scheduler started; agent/plugin paths that still \
         require desktop-only state will return a runtime-host error"
    );

    // Block forever — the shim is running on background tasks.
    futures::future::pending::<()>().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{db_backend_from, DbBackendConfig};

    fn env(entries: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let entries = entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect::<std::collections::HashMap<_, _>>();
        move |key| entries.get(key).cloned()
    }

    #[test]
    fn sqlite_is_default_even_when_database_url_is_present() {
        let config = db_backend_from(env(&[("DATABASE_URL", "postgres://ambient")])).unwrap();
        assert_eq!(config, DbBackendConfig::Sqlite);
    }

    #[test]
    fn postgres_requires_explicit_backend_url_and_tenant() {
        let err = db_backend_from(env(&[("ORBIT_DB_BACKEND", "postgres")])).unwrap_err();
        assert!(err.to_string().contains("ORBIT_POSTGRES_URL"));

        let err = db_backend_from(env(&[
            ("ORBIT_DB_BACKEND", "postgres"),
            ("ORBIT_POSTGRES_URL", "postgres://app"),
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("ORBIT_TENANT_ID"));
    }

    #[test]
    fn postgres_uses_orbit_url_before_database_url() {
        let config = db_backend_from(env(&[
            ("ORBIT_DB_BACKEND", "postgres"),
            ("ORBIT_POSTGRES_URL", "postgres://orbit"),
            ("DATABASE_URL", "postgres://ambient"),
            ("ORBIT_TENANT_ID", "tenant_a"),
        ]))
        .unwrap();
        assert_eq!(
            config,
            DbBackendConfig::Postgres {
                database_url: "postgres://orbit".to_string(),
                tenant_id: "tenant_a".to_string(),
                migrations_url: None,
            }
        );
    }

    #[test]
    fn postgres_allows_database_url_after_explicit_backend_opt_in() {
        let config = db_backend_from(env(&[
            ("ORBIT_DB_BACKEND", "postgres"),
            ("DATABASE_URL", "postgres://fallback"),
            ("ORBIT_TENANT_ID", "tenant_a"),
        ]))
        .unwrap();
        assert_eq!(
            config,
            DbBackendConfig::Postgres {
                database_url: "postgres://fallback".to_string(),
                tenant_id: "tenant_a".to_string(),
                migrations_url: None,
            }
        );
    }

    #[test]
    fn migrations_require_owner_url() {
        let err = db_backend_from(env(&[
            ("ORBIT_DB_BACKEND", "postgres"),
            ("ORBIT_POSTGRES_URL", "postgres://app"),
            ("ORBIT_TENANT_ID", "tenant_a"),
            ("ORBIT_APPLY_POSTGRES_MIGRATIONS", "1"),
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("ORBIT_POSTGRES_MIGRATIONS_URL"));
    }
}
