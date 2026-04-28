//! Standalone Orbit engine server.
//!
//! Boots the same `orbit-engine` crate that the Tauri desktop app uses, but
//! without a Tauri runtime. Reads config from environment variables, opens a
//! SQLite (or, later, Postgres) data layer, spins up the executor +
//! scheduler + plugin manager, and exposes everything over the existing
//! HTTP+WS shim.
//!
//! Status: **skeleton**. The full bootstrap path is blocked on lifting
//! `tauri::AppHandle` from the engine signature behind a runtime-host trait
//! (tracked as Phase A.7 in `plans/quirky-jingling-candy.md`). Until that
//! lands, this binary exists only to claim the workspace slot and validate
//! that `orbit-engine` builds without the `desktop` feature.

use anyhow::{Context, Result};

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,orbit_engine=info,orbit_server=info"));
    fmt().with_env_filter(filter).with_target(false).init();
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "orbit-server starting (skeleton — engine bootstrap blocked on Phase A.7 AppHandle refactor)"
    );

    // Surface a few env vars now so misconfiguration is visible early once
    // the bootstrap lands. None of these are read yet.
    let _database_url = std::env::var("DATABASE_URL").ok();
    let _bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8765".to_string());
    let _workspace_dir = std::env::var("WORKSPACE_DIR")
        .ok()
        .or_else(|| std::env::var("HOME").ok().map(|h| format!("{h}/.orbit")))
        .context("WORKSPACE_DIR or HOME must be set")?;

    tracing::warn!(
        "engine bootstrap not implemented yet — exiting cleanly. \
         Track progress on Phase A.7 (AppHandle decoupling) before adding \
         the actual ExecutorEngine / SchedulerEngine / PluginManager wire-up."
    );

    Ok(())
}
