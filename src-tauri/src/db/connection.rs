use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::path::PathBuf;
use tracing::info;

const MIGRATION_1: &str = include_str!("migrations/0001_initial.sql");
const MIGRATION_2: &str = include_str!("migrations/0002_agent_loop.sql");
const MIGRATION_3: &str = include_str!("migrations/0003_chat_sessions.sql");

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

    info!("Database initialised at {:?}", db_path);
    Ok(DbPool(pool))
}
