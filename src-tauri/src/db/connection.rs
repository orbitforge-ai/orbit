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
const MIGRATION_12: &str = include_str!("migrations/0012_projects.sql");
const MIGRATION_13: &str = include_str!("migrations/0013_compaction_improvements.sql");
const MIGRATION_14: &str = include_str!("migrations/0014_message_reactions.sql");
const MIGRATION_15: &str = include_str!("migrations/0015_session_allow_subagents.sql");
const MIGRATION_16: &str = include_str!("migrations/0016_session_worktrees.sql");
const MIGRATION_17: &str = include_str!("migrations/0017_agent_tasks.sql");
const MIGRATION_18: &str = include_str!("migrations/0018_work_items.sql");
const MIGRATION_19: &str = include_str!("migrations/0019_work_item_comments.sql");
const MIGRATION_20: &str = include_str!("migrations/0020_project_workflows.sql");
const MIGRATION_21: &str = include_str!("migrations/0021_workflow_runs.sql");
const MIGRATION_22: &str = include_str!("migrations/0022_workflow_sources_and_board_columns.sql");
const MIGRATION_23: &str = include_str!("migrations/0023_customizable_project_board.sql");

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
    if version < 12 {
        conn.execute_batch(MIGRATION_12)?;
        conn.execute_batch("PRAGMA user_version = 12;")?;
        info!("Applied migration 12 (projects)");
    }
    if version < 13 {
        conn.execute_batch(MIGRATION_13)?;
        conn.execute_batch("PRAGMA user_version = 13;")?;
        info!("Applied migration 13 (compaction_improvements)");
    }
    if version < 14 {
        conn.execute_batch(MIGRATION_14)?;
        conn.execute_batch("PRAGMA user_version = 14;")?;
        info!("Applied migration 14 (message_reactions)");
    }
    if version < 15 {
        conn.execute_batch(MIGRATION_15)?;
        conn.execute_batch("PRAGMA user_version = 15;")?;
        info!("Applied migration 15 (session_allow_subagents)");
    }
    if version < 16 {
        conn.execute_batch(MIGRATION_16)?;
        conn.execute_batch("PRAGMA user_version = 16;")?;
        info!("Applied migration 16 (session_worktrees)");
    }
    if version < 17 {
        conn.execute_batch(MIGRATION_17)?;
        conn.execute_batch("PRAGMA user_version = 17;")?;
        info!("Applied migration 17 (agent_tasks)");
    }
    if version < 18 {
        conn.execute_batch(MIGRATION_18)?;
        conn.execute_batch("PRAGMA user_version = 18;")?;
        info!("Applied migration 18 (work_items)");
    }
    if version < 19 {
        conn.execute_batch(MIGRATION_19)?;
        conn.execute_batch("PRAGMA user_version = 19;")?;
        info!("Applied migration 19 (work_item_comments)");
    }
    if version < 20 {
        conn.execute_batch(MIGRATION_20)?;
        conn.execute_batch("PRAGMA user_version = 20;")?;
        info!("Applied migration 20 (project_workflows)");
    }
    if version < 21 {
        conn.execute_batch(MIGRATION_21)?;
        conn.execute_batch("PRAGMA user_version = 21;")?;
        info!("Applied migration 21 (workflow_runs + schedules rebuild)");
    }
    if version < 22 {
        conn.execute_batch(MIGRATION_22)?;
        conn.execute_batch("PRAGMA user_version = 22;")?;
        info!("Applied migration 22 (workflow sources + board columns)");
    }
    if version < 23 {
        conn.execute_batch(MIGRATION_23)?;
        conn.execute_batch("PRAGMA user_version = 23;")?;
        info!("Applied migration 23 (customizable project board)");
    }

    info!("Database initialised at {:?}", db_path);
    Ok(DbPool(pool))
}
