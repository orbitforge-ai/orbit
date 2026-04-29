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
const MIGRATION_24: &str = include_str!("migrations/0024_plugin_entities.sql");
const MIGRATION_25: &str = include_str!("migrations/0025_channel_sessions.sql");
const MIGRATION_26: &str = include_str!("migrations/0026_reset_pulse.sql");
const MIGRATION_27: &str = include_str!("migrations/0027_session_skill_state.sql");
const MIGRATION_28: &str = include_str!("migrations/0028_project_boards.sql");
const MIGRATION_29: &str = include_str!("migrations/0029_board_scoped_default_column.sql");
const MIGRATION_30: &str = include_str!("migrations/0030_work_item_events.sql");
const MIGRATION_31: &str = include_str!("migrations/0031_tenant_id.sql");

/// Newtype wrapper — stored as Tauri managed state.
/// The compatibility `Pool` wraps `sqlx::SqlitePool`, which is Arc-based
/// internally and cheap to clone.
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
    if version < 24 {
        conn.execute_batch(MIGRATION_24)?;
        conn.execute_batch("PRAGMA user_version = 24;")?;
        info!("Applied migration 24 (plugin entities + workflow subscriptions)");
    }
    if version < 25 {
        conn.execute_batch(MIGRATION_25)?;
        conn.execute_batch("PRAGMA user_version = 25;")?;
        info!("Applied migration 25 (channel sessions)");
    }
    if version < 26 {
        conn.execute_batch(MIGRATION_26)?;
        conn.execute_batch("PRAGMA user_version = 26;")?;
        info!("Applied migration 26 (reset pulse for project scoping)");
    }
    if version < 27 {
        conn.execute_batch(MIGRATION_27)?;
        conn.execute_batch("PRAGMA user_version = 27;")?;
        info!("Applied migration 27 (session skill state)");
    }
    if version < 28 {
        conn.execute_batch(MIGRATION_28)?;
        backfill_default_project_boards(&conn)?;
        conn.execute_batch("PRAGMA user_version = 28;")?;
        info!("Applied migration 28 (project boards)");
    }
    if version < 29 {
        conn.execute_batch(MIGRATION_29)?;
        conn.execute_batch("PRAGMA user_version = 29;")?;
        info!("Applied migration 29 (board-scoped default column index)");
    }
    if version < 30 {
        conn.execute_batch(MIGRATION_30)?;
        conn.execute_batch("PRAGMA user_version = 30;")?;
        info!("Applied migration 30 (work_item_events)");
    }
    if version < 31 {
        conn.execute_batch(MIGRATION_31)?;
        conn.execute_batch("PRAGMA user_version = 31;")?;
        info!("Applied migration 31 (tenant_id on every table)");
    }

    info!("Database initialised at {:?}", db_path);
    Ok(DbPool(pool))
}

/// Creates one default board per project that doesn't have one yet, then
/// re-parents every legacy `project_board_columns` / `work_items` row to
/// that board. Runs at migration 28 apply time (and is idempotent: projects
/// that already have a default board are skipped).
fn backfill_default_project_boards(
    conn: &rusqlite::Connection,
) -> Result<(), Box<dyn std::error::Error>> {
    use rusqlite::params;
    use ulid::Ulid;

    let now = chrono::Utc::now().to_rfc3339();

    let mut project_stmt = conn.prepare(
        "SELECT p.id, p.name
           FROM projects p
          WHERE NOT EXISTS (
              SELECT 1 FROM project_boards b
               WHERE b.project_id = p.id
                 AND b.is_default = 1
          )",
    )?;
    let projects: Vec<(String, String)> = project_stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();
    drop(project_stmt);

    for (project_id, project_name) in projects {
        let board_id = Ulid::new().to_string();
        let prefix = derive_default_board_prefix(conn, &project_id, &project_name)?;

        conn.execute(
            "INSERT INTO project_boards (id, project_id, name, prefix, position, is_default, created_at, updated_at)
             VALUES (?1, ?2, 'Default', ?3, 1024.0, 1, ?4, ?4)",
            params![board_id, project_id, prefix, now],
        )?;

        conn.execute(
            "UPDATE project_board_columns
                SET board_id = ?1
              WHERE project_id = ?2 AND board_id IS NULL",
            params![board_id, project_id],
        )?;

        conn.execute(
            "UPDATE work_items
                SET board_id = ?1
              WHERE project_id = ?2 AND board_id IS NULL",
            params![board_id, project_id],
        )?;
    }

    Ok(())
}

fn derive_default_board_prefix(
    conn: &rusqlite::Connection,
    project_id: &str,
    project_name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    use rusqlite::params;

    let base: String = project_name
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .map(|c| c.to_ascii_uppercase())
        .take(4)
        .collect();
    let base = if base.len() >= 2 {
        base
    } else {
        "MAIN".to_string()
    };

    // Uniqueness is scoped per project, and we only create one default board
    // per project here — so a collision can only happen if an earlier run
    // already created a board with the same prefix. Append A, B, C, ... in
    // that case. Bounded to 26 attempts; ULID fallback guarantees termination.
    for suffix in std::iter::once(String::new()).chain(('A'..='Z').map(|c| c.to_string())) {
        let candidate = format!("{}{}", base, suffix);
        if candidate.len() > 8 {
            continue;
        }
        let taken: bool = conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM project_boards
                 WHERE project_id = ?1 AND prefix = ?2
             )",
            params![project_id, candidate],
            |row| row.get(0),
        )?;
        if !taken {
            return Ok(candidate);
        }
    }
    Ok(format!("B{}", &ulid::Ulid::new().to_string()[..3]))
}
