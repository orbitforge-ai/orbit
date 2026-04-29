//! Standalone Orbit engine server.
//!
//! Boots the same `orbit-engine` crate that the Tauri desktop app uses, but
//! without a Tauri runtime. Reads config from environment variables, opens a
//! SQLite (or, later, Postgres) data layer, constructs an `AppContext` with
//! `tauri: None`, and exposes everything over the existing HTTP+WS shim.
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
use orbit_engine::db::repos::{sqlite::SqliteRepos, Repos};
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

    // Repository facade. Sqlite-backed during the migration; once Phase C
    // lands the Postgres impl this becomes a runtime decision based on
    // `DATABASE_URL`.
    let repos: Arc<dyn Repos> = Arc::new(SqliteRepos::new(db_pool.clone()));

    // ── AppContext ──────────────────────────────────────────────────────────
    let ctx = AppContext::new(
        db_pool,
        repos,
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
    let engine = ExecutorEngine::new(
        ctx.db.clone(),
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

    let scheduler = SchedulerEngine::new(
        ctx.db.clone(),
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
