use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::path::PathBuf;
use tracing::{info, warn};

const MIGRATION_1: &str = include_str!("migrations/0001_initial.sql");
const MIGRATION_2: &str = include_str!("migrations/0002_agent_loop.sql");
const MIGRATION_3: &str = include_str!("migrations/0003_chat_sessions.sql");
const MIGRATION_4: &str = include_str!("migrations/0004_agent_slugs.sql");
const MIGRATION_5: &str = include_str!("migrations/0005_context_management.sql");
const MIGRATION_6: &str = include_str!("migrations/0006_agent_bus.sql");
const MIGRATION_7: &str = include_str!("migrations/0007_sub_agents.sql");
const MIGRATION_8: &str = include_str!("migrations/0008_session_workflows.sql");
const MIGRATION_9: &str = include_str!("migrations/0009_drop_sessions.sql");
const MIGRATION_10: &str = include_str!("migrations/0010_users.sql");
const MIGRATION_11: &str = include_str!("migrations/0011_memory_extraction_log.sql");

/// Newtype wrapper — stored as Tauri managed state.
/// r2d2::Pool is Arc-based internally: cheap to clone.
#[derive(Clone)]
pub struct DbPool(pub Pool<SqliteConnectionManager>);

impl DbPool {
    pub fn get(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, r2d2::Error> {
        self.0.get()
    }
}

pub fn init(data_dir: PathBuf) -> Result<DbPool, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("orbit.db");

    let manager = SqliteConnectionManager::file(&db_path);
    let pool = Pool::builder().max_size(8).build(manager)?;

    let conn = pool.get()?;

    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap_or(0);

    if version < 1 {
        conn.execute_batch(MIGRATION_1)?;
        conn.execute_batch("PRAGMA user_version = 1;")?;
        info!("Applied migration 1 (initial schema)");
    }
    if version < 2 {
        conn.execute_batch(MIGRATION_2)?;
        conn.execute_batch("PRAGMA user_version = 2;")?;
        info!("Applied migration 2 (agent_loop)");
    }
    if version < 3 {
        conn.execute_batch(MIGRATION_3)?;
        conn.execute_batch("PRAGMA user_version = 3;")?;
        info!("Applied migration 3 (chat_sessions)");
    }
    if version < 4 {
        conn.execute_batch(MIGRATION_4)?;
        conn.execute_batch("PRAGMA user_version = 4;")?;
        // Rename workspace directory on disk
        let agents_dir = data_dir.join("agents");
        let old_dir = agents_dir.join("01HZDEFAULTDEFAULTDEFAULTDA");
        let new_dir = agents_dir.join("default");
        if old_dir.exists() && !new_dir.exists() {
            if let Err(e) = std::fs::rename(&old_dir, &new_dir) {
                warn!("Failed to rename default agent workspace: {}", e);
            } else {
                info!("Renamed default agent workspace to 'default'");
            }
        }
        info!("Applied migration 4 (agent_slugs)");
    }
    if version < 5 {
        conn.execute_batch(MIGRATION_5)?;
        conn.execute_batch("PRAGMA user_version = 5;")?;
        info!("Applied migration 5 (context_management)");
    }
    if version < 6 {
        conn.execute_batch(MIGRATION_6)?;
        conn.execute_batch("PRAGMA user_version = 6;")?;
        info!("Applied migration 6 (agent_bus)");
    }
    if version < 7 {
        conn.execute_batch(MIGRATION_7)?;
        conn.execute_batch("PRAGMA user_version = 7;")?;
        info!("Applied migration 7 (sub_agents)");
    }
    if version < 8 {
        conn.execute_batch(MIGRATION_8)?;
        conn.execute_batch("PRAGMA user_version = 8;")?;
        info!("Applied migration 8 (session_workflows)");
    }
    if version < 9 {
        conn.execute_batch(MIGRATION_9)?;
        conn.execute_batch("PRAGMA user_version = 9;")?;
        info!("Applied migration 9 (drop_sessions)");
    }
    if version < 10 {
        conn.execute_batch(MIGRATION_10)?;
        conn.execute_batch("PRAGMA user_version = 10;")?;
        info!("Applied migration 10 (users)");
    }
    if version < 11 {
        conn.execute_batch(MIGRATION_11)?;
        conn.execute_batch("PRAGMA user_version = 11;")?;
        info!("Applied migration 11 (memory_extraction_log)");
    }

    info!("Database initialised at {:?}", db_path);
    Ok(DbPool(pool))
}
