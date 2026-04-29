//! SQLite-backed `Repos` impl built on the existing rusqlite/r2d2 pool.
//!
//! Until rusqlite is removed, this is what every desktop and per-tenant-VM
//! deployment runs. Queries are lifted verbatim from the original
//! `commands/{tasks,…}.rs` so behaviour is identical — the only architectural
//! change is that they're reachable through the `Repos` trait instead of via
//! `tauri::State<DbPool>`.

use async_trait::async_trait;
use rusqlite::{params, OptionalExtension};
use ulid::Ulid;

use crate::db::repos::{AgentRepo, ProjectRepo, Repos, ScheduleRepo, TaskRepo, UserRepo};
use crate::db::DbPool;
use crate::executor::workspace;
use crate::models::agent::{Agent, CreateAgent, UpdateAgent};
use crate::models::project::{CreateProject, Project, ProjectSummary, UpdateProject};
use crate::models::schedule::{CreateSchedule, RecurringConfig, Schedule};
use crate::models::task::{CreateTask, Task, UpdateTask};
use crate::models::user::User;
use crate::scheduler::converter::{next_n_runs, to_cron};

// ── Boilerplate-killers ─────────────────────────────────────────────────────
//
// Every rusqlite call ends up doing the same dance: borrow a connection from
// the pool inside a `spawn_blocking`, surface the errors as `String`, and
// hand the connection to a closure. The two helpers below encapsulate that
// dance so each repo method can read as just its query — and any aggregate
// that lands later can be implemented in a few lines instead of fifteen.

/// Convert any error type that implements `Display` into the `String` shape
/// the repo trait surface uses. Saves the recurring `.map_err(|e| e.to_string())`.
trait IntoStringErr<T> {
    fn err_str(self) -> Result<T, String>;
}

impl<T, E: std::fmt::Display> IntoStringErr<T> for Result<T, E> {
    fn err_str(self) -> Result<T, String> {
        self.map_err(|e| e.to_string())
    }
}

#[derive(Clone)]
pub struct SqliteRepos {
    pool: DbPool,
}

impl SqliteRepos {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    /// Run a closure on a pooled rusqlite `Connection` from a blocking-task
    /// thread. Closure failures (any `Display`-able error) are surfaced as
    /// `String`. Use this for read paths and single-statement writes.
    async fn with_conn<T, F>(&self, f: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&rusqlite::Connection) -> Result<T, String> + Send + 'static,
    {
        let pool = self.pool.0.clone();
        tokio::task::spawn_blocking(move || -> Result<T, String> {
            let conn = pool.get().err_str()?;
            f(&conn)
        })
        .await
        .err_str()?
    }

    /// Like `with_conn` but yields a `&mut Connection` so the closure can
    /// open a transaction.
    #[allow(dead_code)]
    async fn with_conn_mut<T, F>(&self, f: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&mut rusqlite::Connection) -> Result<T, String> + Send + 'static,
    {
        let pool = self.pool.0.clone();
        tokio::task::spawn_blocking(move || -> Result<T, String> {
            let mut conn = pool.get().err_str()?;
            f(&mut conn)
        })
        .await
        .err_str()?
    }
}

impl Repos for SqliteRepos {
    fn agents(&self) -> &dyn AgentRepo {
        self
    }
    fn projects(&self) -> &dyn ProjectRepo {
        self
    }
    fn schedules(&self) -> &dyn ScheduleRepo {
        self
    }
    fn tasks(&self) -> &dyn TaskRepo {
        self
    }
    fn users(&self) -> &dyn UserRepo {
        self
    }
}

const TASK_COLUMNS: &str = "id, name, description, kind, config, max_duration_seconds, max_retries,
            retry_delay_seconds, concurrency_policy, tags, agent_id,
            enabled, created_at, updated_at, project_id";

fn map_task_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let config_str: String = row.get(4)?;
    let tags_str: String = row.get(9)?;
    Ok(Task {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        kind: row.get(3)?,
        config: serde_json::from_str(&config_str).unwrap_or(serde_json::Value::Null),
        max_duration_seconds: row.get(5)?,
        max_retries: row.get(6)?,
        retry_delay_seconds: row.get(7)?,
        concurrency_policy: row.get(8)?,
        tags: serde_json::from_str(&tags_str).unwrap_or_default(),
        agent_id: row.get(10)?,
        enabled: row.get::<_, bool>(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        project_id: row.get(14)?,
    })
}

#[async_trait]
impl TaskRepo for SqliteRepos {
    async fn list(&self) -> Result<Vec<Task>, String> {
        self.with_conn(|conn| {
            // Newest first so the dashboard shows recent activity at the top.
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {TASK_COLUMNS} FROM tasks ORDER BY created_at DESC"
                ))
                .err_str()?;
            let rows: Vec<Task> = stmt
                .query_map([], map_task_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn get(&self, id: &str) -> Result<Option<Task>, String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            conn.query_row(
                &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1"),
                params![id],
                map_task_row,
            )
            .optional()
            .err_str()
        })
        .await
    }

    async fn create(&self, payload: CreateTask) -> Result<Task, String> {
        self.with_conn(move |conn| {
            // Default scalars stay close to where the row is built so future
            // schema additions can be wired in one place.
            let id = Ulid::new().to_string();
            let now = chrono::Utc::now().to_rfc3339();
            let config_str = serde_json::to_string(&payload.config).err_str()?;
            let tags_str =
                serde_json::to_string(&payload.tags.unwrap_or_default()).err_str()?;
            let max_duration = payload.max_duration_seconds.unwrap_or(3600);
            let max_retries = payload.max_retries.unwrap_or(0);
            let retry_delay = payload.retry_delay_seconds.unwrap_or(60);
            let concurrency = payload
                .concurrency_policy
                .unwrap_or_else(|| "allow".to_string());
            let agent_id = payload.agent_id.unwrap_or_else(|| "default".to_string());

            conn.execute(
                "INSERT INTO tasks (id, name, description, kind, config, max_duration_seconds,
                                    max_retries, retry_delay_seconds, concurrency_policy, tags,
                                    agent_id, enabled, created_at, updated_at, project_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, ?12, ?12, ?13)",
                params![
                    id,
                    payload.name,
                    payload.description,
                    payload.kind,
                    config_str,
                    max_duration,
                    max_retries,
                    retry_delay,
                    concurrency,
                    tags_str,
                    agent_id,
                    now,
                    payload.project_id
                ],
            )
            .err_str()?;

            conn.query_row(
                &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1"),
                params![id],
                map_task_row,
            )
            .err_str()
        })
        .await
    }

    async fn update(&self, id: &str, payload: UpdateTask) -> Result<Task, String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();

            // Each Some-field becomes a single-column UPDATE. SQLite is fine
            // with this many round-trips for an edit dialog. If the surface
            // grows we can switch to a builder, but the trait surface stays
            // the same.
            macro_rules! patch {
                ($column:expr, $value:expr) => {
                    conn.execute(
                        &format!("UPDATE tasks SET {} = ?1, updated_at = ?2 WHERE id = ?3", $column),
                        params![$value, now, id],
                    )
                    .err_str()?;
                };
            }
            if let Some(name) = &payload.name {
                patch!("name", name);
            }
            if let Some(desc) = &payload.description {
                patch!("description", desc);
            }
            if let Some(cfg) = &payload.config {
                let s = serde_json::to_string(cfg).err_str()?;
                patch!("config", s);
            }
            if let Some(enabled) = payload.enabled {
                patch!("enabled", enabled as i64);
            }
            if let Some(agent_id) = &payload.agent_id {
                patch!("agent_id", agent_id);
            }
            if let Some(max_duration) = payload.max_duration_seconds {
                patch!("max_duration_seconds", max_duration);
            }
            if let Some(max_retries) = payload.max_retries {
                patch!("max_retries", max_retries);
            }
            if let Some(retry_delay) = payload.retry_delay_seconds {
                patch!("retry_delay_seconds", retry_delay);
            }
            if let Some(policy) = &payload.concurrency_policy {
                patch!("concurrency_policy", policy);
            }
            if let Some(tags) = &payload.tags {
                let t = serde_json::to_string(tags).err_str()?;
                patch!("tags", t);
            }
            if let Some(project_id) = &payload.project_id {
                // Empty string sentinel means "clear the project FK".
                let pid: Option<&str> = if project_id.is_empty() {
                    None
                } else {
                    Some(project_id.as_str())
                };
                patch!("project_id", pid);
            }

            conn.query_row(
                &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1"),
                params![id],
                map_task_row,
            )
            .err_str()
        })
        .await
    }

    async fn delete(&self, id: &str) -> Result<(), String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])
                .err_str()?;
            Ok(())
        })
        .await
    }
}

// ── Agents ──────────────────────────────────────────────────────────────────

const AGENT_COLUMNS: &str =
    "id, name, description, state, max_concurrent_runs, heartbeat_at, created_at, updated_at";

fn map_agent_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Agent> {
    Ok(Agent {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        state: row.get(3)?,
        max_concurrent_runs: row.get(4)?,
        heartbeat_at: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

#[async_trait]
impl AgentRepo for SqliteRepos {
    async fn list(&self) -> Result<Vec<Agent>, String> {
        self.with_conn(|conn| {
            // Oldest first matches the existing UI order in the agent picker.
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {AGENT_COLUMNS} FROM agents ORDER BY created_at ASC"
                ))
                .err_str()?;
            let rows: Vec<Agent> = stmt
                .query_map([], map_agent_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn get(&self, id: &str) -> Result<Option<Agent>, String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            conn.query_row(
                &format!("SELECT {AGENT_COLUMNS} FROM agents WHERE id = ?1"),
                params![id],
                map_agent_row,
            )
            .optional()
            .err_str()
        })
        .await
    }

    async fn create_basic(&self, payload: CreateAgent) -> Result<Agent, String> {
        self.with_conn(move |conn| {
            // Slug-style ID derived from the agent's display name; collisions
            // get a numeric suffix.
            let id = next_available_agent_id_inner(conn, &payload.name, None)?;
            let now = chrono::Utc::now().to_rfc3339();
            let max_runs = payload.max_concurrent_runs.unwrap_or(5);

            conn.execute(
                "INSERT INTO agents (id, name, description, state, max_concurrent_runs, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'idle', ?4, ?5, ?5)",
                params![id, payload.name, payload.description, max_runs, now],
            )
            .err_str()?;

            conn.query_row(
                &format!("SELECT {AGENT_COLUMNS} FROM agents WHERE id = ?1"),
                params![id],
                map_agent_row,
            )
            .err_str()
        })
        .await
    }

    async fn set_model_config(&self, id: &str, model_config_json: &str) -> Result<(), String> {
        let id = id.to_string();
        let mc = model_config_json.to_string();
        self.with_conn(move |conn| {
            conn.execute(
                "UPDATE agents SET model_config = ?1 WHERE id = ?2",
                params![mc, id],
            )
            .err_str()?;
            Ok(())
        })
        .await
    }

    async fn update_basic(&self, id: &str, payload: UpdateAgent) -> Result<Agent, String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            // Same Some-field-per-UPDATE shape as TaskRepo::update.
            macro_rules! patch {
                ($column:expr, $value:expr) => {
                    conn.execute(
                        &format!("UPDATE agents SET {} = ?1, updated_at = ?2 WHERE id = ?3", $column),
                        params![$value, now, id],
                    )
                    .err_str()?;
                };
            }
            if let Some(name) = &payload.name {
                patch!("name", name);
            }
            if let Some(desc) = &payload.description {
                patch!("description", desc);
            }
            if let Some(max_runs) = payload.max_concurrent_runs {
                patch!("max_concurrent_runs", max_runs);
            }
            conn.query_row(
                &format!("SELECT {AGENT_COLUMNS} FROM agents WHERE id = ?1"),
                params![id],
                map_agent_row,
            )
            .err_str()
        })
        .await
    }

    async fn delete(&self, id: &str) -> Result<(), String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            conn.execute("DELETE FROM agents WHERE id = ?1", params![id])
                .err_str()?;
            Ok(())
        })
        .await
    }

    async fn next_available_id(
        &self,
        name: &str,
        current_id: Option<&str>,
    ) -> Result<String, String> {
        let name = name.to_string();
        let current_id = current_id.map(|s| s.to_string());
        self.with_conn(move |conn| {
            next_available_agent_id_inner(conn, &name, current_id.as_deref())
        })
        .await
    }
}

fn next_available_agent_id_inner(
    conn: &rusqlite::Connection,
    name: &str,
    current_id: Option<&str>,
) -> Result<String, String> {
    let base_slug = workspace::slugify(name);
    let base_slug = if base_slug.is_empty() {
        "agent".to_string()
    } else {
        base_slug
    };
    let mut candidate = base_slug.clone();
    let mut suffix = 1;
    loop {
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM agents WHERE id = ?1",
                params![candidate],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        match existing.as_deref() {
            None => return Ok(candidate),
            Some(existing_id) if Some(existing_id) == current_id => return Ok(candidate),
            Some(_) => {
                suffix += 1;
                candidate = format!("{}-{}", base_slug, suffix);
            }
        }
    }
}

// ── Projects ────────────────────────────────────────────────────────────────

fn map_project_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

fn map_project_summary_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectSummary> {
    Ok(ProjectSummary {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        agent_count: row.get(5)?,
    })
}

#[async_trait]
impl ProjectRepo for SqliteRepos {
    async fn list(&self) -> Result<Vec<ProjectSummary>, String> {
        self.with_conn(|conn| {
            // Single LEFT JOIN gives us each project's agent count without
            // an N+1 follow-up query.
            let mut stmt = conn
                .prepare(
                    "SELECT p.id, p.name, p.description, p.created_at, p.updated_at,
                            COALESCE(pa.agent_count, 0) AS agent_count
                     FROM projects p
                     LEFT JOIN (
                         SELECT project_id, COUNT(*) AS agent_count
                         FROM project_agents
                         GROUP BY project_id
                     ) pa ON pa.project_id = p.id
                     ORDER BY p.created_at ASC",
                )
                .err_str()?;
            let rows: Vec<ProjectSummary> = stmt
                .query_map([], map_project_summary_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn get(&self, id: &str) -> Result<Option<Project>, String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            conn.query_row(
                "SELECT id, name, description, created_at, updated_at FROM projects WHERE id = ?1",
                params![id],
                map_project_row,
            )
            .optional()
            .err_str()
        })
        .await
    }

    async fn create_basic(&self, payload: CreateProject) -> Result<Project, String> {
        self.with_conn(move |conn| {
            // Slug + collision-resolving suffix, same shape as agent IDs.
            let base_slug = workspace::slugify(&payload.name);
            let base_slug = if base_slug.is_empty() {
                "project".to_string()
            } else {
                base_slug
            };
            let mut candidate = base_slug.clone();
            let mut suffix = 1;
            while conn
                .query_row(
                    "SELECT 1 FROM projects WHERE id = ?1",
                    params![candidate],
                    |_| Ok(()),
                )
                .is_ok()
            {
                suffix += 1;
                candidate = format!("{}-{}", base_slug, suffix);
            }
            let id = candidate;

            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO projects (id, name, description, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                params![id, payload.name, payload.description, now],
            )
            .err_str()?;

            conn.query_row(
                "SELECT id, name, description, created_at, updated_at FROM projects WHERE id = ?1",
                params![id],
                map_project_row,
            )
            .err_str()
        })
        .await
    }

    async fn update(&self, id: &str, payload: UpdateProject) -> Result<Project, String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            if let Some(name) = &payload.name {
                conn.execute(
                    "UPDATE projects SET name = ?1, updated_at = ?2 WHERE id = ?3",
                    params![name, now, id],
                )
                .err_str()?;
            }
            if let Some(desc) = &payload.description {
                conn.execute(
                    "UPDATE projects SET description = ?1, updated_at = ?2 WHERE id = ?3",
                    params![desc, now, id],
                )
                .err_str()?;
            }
            conn.query_row(
                "SELECT id, name, description, created_at, updated_at FROM projects WHERE id = ?1",
                params![id],
                map_project_row,
            )
            .err_str()
        })
        .await
    }

    async fn delete(&self, id: &str) -> Result<(), String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            conn.execute("DELETE FROM projects WHERE id = ?1", params![id])
                .err_str()?;
            Ok(())
        })
        .await
    }
}

// ── Schedules ───────────────────────────────────────────────────────────────

const SCHEDULE_COLUMNS: &str =
    "id, task_id, workflow_id, target_kind, kind, config, enabled, \
     next_run_at, last_run_at, created_at, updated_at";

fn map_schedule_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Schedule> {
    let config_str: String = row.get(5)?;
    Ok(Schedule {
        id: row.get(0)?,
        task_id: row.get(1)?,
        workflow_id: row.get(2)?,
        target_kind: row.get(3)?,
        kind: row.get(4)?,
        config: serde_json::from_str(&config_str).unwrap_or(serde_json::Value::Null),
        enabled: row.get::<_, bool>(6)?,
        next_run_at: row.get(7)?,
        last_run_at: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

#[async_trait]
impl ScheduleRepo for SqliteRepos {
    async fn list(&self) -> Result<Vec<Schedule>, String> {
        let pool = self.pool.0.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Schedule>, String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            let sql = format!("SELECT {SCHEDULE_COLUMNS} FROM schedules ORDER BY created_at DESC");
            let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
            let schedules = stmt
                .query_map([], map_schedule_row)
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();
            Ok(schedules)
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn list_for_task(&self, task_id: &str) -> Result<Vec<Schedule>, String> {
        let pool = self.pool.0.clone();
        let task_id = task_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<Schedule>, String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            let sql = format!("SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE task_id = ?1");
            let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
            let schedules = stmt
                .query_map(params![task_id], map_schedule_row)
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();
            Ok(schedules)
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn list_for_workflow(&self, workflow_id: &str) -> Result<Vec<Schedule>, String> {
        let pool = self.pool.0.clone();
        let workflow_id = workflow_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<Schedule>, String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            let sql = format!("SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE workflow_id = ?1");
            let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
            let schedules = stmt
                .query_map(params![workflow_id], map_schedule_row)
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();
            Ok(schedules)
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn create(&self, payload: CreateSchedule) -> Result<Schedule, String> {
        let pool = self.pool.0.clone();
        tokio::task::spawn_blocking(move || -> Result<Schedule, String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            let id = Ulid::new().to_string();
            let now = chrono::Utc::now().to_rfc3339();
            let config_str = serde_json::to_string(&payload.config).map_err(|e| e.to_string())?;

            let target_kind = payload
                .target_kind
                .clone()
                .unwrap_or_else(|| "task".to_string());
            if target_kind != "task" && target_kind != "workflow" {
                return Err(format!("invalid target_kind: {}", target_kind));
            }
            match target_kind.as_str() {
                "task" => {
                    if payload.task_id.is_none() || payload.workflow_id.is_some() {
                        return Err("task schedule requires task_id and no workflow_id".into());
                    }
                }
                "workflow" => {
                    if payload.workflow_id.is_none() || payload.task_id.is_some() {
                        return Err("workflow schedule requires workflow_id and no task_id".into());
                    }
                }
                _ => unreachable!(),
            }

            let next_run_at = if payload.kind == "recurring" {
                let cfg: RecurringConfig = serde_json::from_value(payload.config.clone())
                    .map_err(|e| format!("invalid recurring config: {}", e))?;
                to_cron(&cfg)
                    .ok()
                    .and_then(|_| next_n_runs(&cfg, 1).into_iter().next())
            } else {
                None
            };

            conn.execute(
                "INSERT INTO schedules (id, task_id, workflow_id, target_kind, kind, config, enabled,
                                        next_run_at, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?8)",
                params![
                    id,
                    payload.task_id,
                    payload.workflow_id,
                    target_kind,
                    payload.kind,
                    config_str,
                    next_run_at,
                    now
                ],
            )
            .map_err(|e| e.to_string())?;

            let sql = format!("SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE id = ?1");
            conn.query_row(&sql, params![id], map_schedule_row)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn toggle(&self, id: &str, enabled: bool) -> Result<Schedule, String> {
        let pool = self.pool.0.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Schedule, String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE schedules SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
                params![enabled as i64, now, id],
            )
            .map_err(|e| e.to_string())?;
            let sql = format!("SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE id = ?1");
            conn.query_row(&sql, params![id], map_schedule_row)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn delete(&self, id: &str) -> Result<(), String> {
        let pool = self.pool.0.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM schedules WHERE id = ?1", params![id])
                .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

// ── Users ───────────────────────────────────────────────────────────────────

#[async_trait]
impl UserRepo for SqliteRepos {
    async fn list(&self) -> Result<Vec<User>, String> {
        let pool = self.pool.0.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<User>, String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            let mut stmt = conn
                .prepare("SELECT id, name, is_default, created_at FROM users ORDER BY created_at ASC")
                .map_err(|e| e.to_string())?;
            let users = stmt
                .query_map([], |row| {
                    Ok(User {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        is_default: row.get(2)?,
                        created_at: row.get(3)?,
                    })
                })
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();
            Ok(users)
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn create(&self, name: String) -> Result<User, String> {
        let pool = self.pool.0.clone();
        tokio::task::spawn_blocking(move || -> Result<User, String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            let id = Ulid::new().to_string();
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO users (id, name, is_default, created_at) VALUES (?1, ?2, 0, ?3)",
                params![id, name, now],
            )
            .map_err(|e| e.to_string())?;
            Ok(User {
                id,
                name,
                is_default: false,
                created_at: now,
            })
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn exists(&self, id: &str) -> Result<bool, String> {
        let pool = self.pool.0.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> Result<bool, String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM users WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            Ok(count > 0)
        })
        .await
        .map_err(|e| e.to_string())?
    }
}
