use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::path::PathBuf;
use tracing::info;

const MIGRATION: &str = include_str!("migrations/0001_initial.sql");

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

    // Run migrations on first available connection
    let conn = pool.get()?;
    conn.execute_batch(MIGRATION)?;

    info!("Database initialised at {:?}", db_path);
    Ok(DbPool(pool))
}
