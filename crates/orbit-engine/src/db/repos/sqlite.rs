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

use crate::db::repos::{
    AgentRepo, BusMessageRepo, BusSubscriptionRepo, ChatRepo, ChatSessionListFilter,
    ProjectBoardColumnRepo, ProjectBoardRepo, ProjectRepo, ProjectWorkflowRepo, Repos,
    RunListFilter, RunRepo, ScheduleRepo, TaskRepo, UserRepo, WorkItemEventRepo, WorkItemRepo,
    WorkflowRunRepo,
};
use crate::db::DbPool;
use crate::executor::workspace;
use crate::models::agent::{Agent, CreateAgent, UpdateAgent};
use crate::models::bus::{
    BusMessage, BusSubscription, BusThreadMessage, CreateBusSubscription, PaginatedBusThread,
};
use crate::models::chat::{
    ChatMessageRow, ChatMessageRows, ChatSession, ChatSessionMeta, ChatSessionTokenUsage,
    MessageReactionRow, SessionExecutionStatus,
};
use crate::models::project::{CreateProject, Project, ProjectSummary, UpdateProject};
use crate::models::project_board::{
    CreateProjectBoard, DeleteProjectBoard, ProjectBoard, UpdateProjectBoard,
};
use crate::models::project_board_column::ProjectBoardColumn;
use crate::models::project_workflow::ProjectWorkflow;
use crate::models::run::{Run, RunSummary};
use crate::models::schedule::{CreateSchedule, RecurringConfig, Schedule};
use crate::models::task::{CreateTask, Task, UpdateTask};
use crate::models::user::User;
use crate::models::work_item::WorkItem;
use crate::models::work_item_comment::WorkItemComment;
use crate::models::work_item_event::WorkItemEvent;
use crate::models::workflow_run::{WorkflowRun, WorkflowRunSummary, WorkflowRunWithSteps};
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
    fn bus_messages(&self) -> &dyn BusMessageRepo {
        self
    }
    fn bus_subscriptions(&self) -> &dyn BusSubscriptionRepo {
        self
    }
    fn chat(&self) -> &dyn ChatRepo {
        self
    }
    fn project_board_columns(&self) -> &dyn ProjectBoardColumnRepo {
        self
    }
    fn project_boards(&self) -> &dyn ProjectBoardRepo {
        self
    }
    fn project_workflows(&self) -> &dyn ProjectWorkflowRepo {
        self
    }
    fn projects(&self) -> &dyn ProjectRepo {
        self
    }
    fn runs(&self) -> &dyn RunRepo {
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
    fn work_items(&self) -> &dyn WorkItemRepo {
        self
    }
    fn work_item_events(&self) -> &dyn WorkItemEventRepo {
        self
    }
    fn workflow_runs(&self) -> &dyn WorkflowRunRepo {
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
        self.with_conn(|conn| {
            // Newest-first matches the dashboard ordering.
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {SCHEDULE_COLUMNS} FROM schedules ORDER BY created_at DESC"
                ))
                .err_str()?;
            let rows: Vec<Schedule> = stmt
                .query_map([], map_schedule_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn list_for_task(&self, task_id: &str) -> Result<Vec<Schedule>, String> {
        let task_id = task_id.to_string();
        self.with_conn(move |conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE task_id = ?1"
                ))
                .err_str()?;
            let rows: Vec<Schedule> = stmt
                .query_map(params![task_id], map_schedule_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn list_for_workflow(&self, workflow_id: &str) -> Result<Vec<Schedule>, String> {
        let workflow_id = workflow_id.to_string();
        self.with_conn(move |conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE workflow_id = ?1"
                ))
                .err_str()?;
            let rows: Vec<Schedule> = stmt
                .query_map(params![workflow_id], map_schedule_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn create(&self, payload: CreateSchedule) -> Result<Schedule, String> {
        self.with_conn(move |conn| {
            let id = Ulid::new().to_string();
            let now = chrono::Utc::now().to_rfc3339();
            let config_str = serde_json::to_string(&payload.config).err_str()?;

            // Validate target shape. A schedule must point at exactly one of
            // {task, workflow} — never both, never neither.
            let target_kind = payload
                .target_kind
                .clone()
                .unwrap_or_else(|| "task".to_string());
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
                other => return Err(format!("invalid target_kind: {}", other)),
            }

            // Recurring schedules pre-compute their first fire time so the
            // cron worker doesn't have to parse on the hot path.
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
            .err_str()?;

            conn.query_row(
                &format!("SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE id = ?1"),
                params![id],
                map_schedule_row,
            )
            .err_str()
        })
        .await
    }

    async fn toggle(&self, id: &str, enabled: bool) -> Result<Schedule, String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE schedules SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
                params![enabled as i64, now, id],
            )
            .err_str()?;
            conn.query_row(
                &format!("SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE id = ?1"),
                params![id],
                map_schedule_row,
            )
            .err_str()
        })
        .await
    }

    async fn delete(&self, id: &str) -> Result<(), String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            conn.execute("DELETE FROM schedules WHERE id = ?1", params![id])
                .err_str()?;
            Ok(())
        })
        .await
    }
}

// ── Users ───────────────────────────────────────────────────────────────────

fn map_user_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    Ok(User {
        id: row.get(0)?,
        name: row.get(1)?,
        is_default: row.get(2)?,
        created_at: row.get(3)?,
    })
}

#[async_trait]
impl UserRepo for SqliteRepos {
    async fn list(&self) -> Result<Vec<User>, String> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT id, name, is_default, created_at FROM users ORDER BY created_at ASC")
                .err_str()?;
            let rows: Vec<User> = stmt
                .query_map([], map_user_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn create(&self, name: String) -> Result<User, String> {
        self.with_conn(move |conn| {
            let id = Ulid::new().to_string();
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO users (id, name, is_default, created_at) VALUES (?1, ?2, 0, ?3)",
                params![id, name, now],
            )
            .err_str()?;
            Ok(User {
                id,
                name,
                is_default: false,
                created_at: now,
            })
        })
        .await
    }

    async fn exists(&self, id: &str) -> Result<bool, String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM users WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .err_str()?;
            Ok(count > 0)
        })
        .await
    }
}

// ── Runs ────────────────────────────────────────────────────────────────────
//
// Read-only at this layer. The "summary" SQL is shared between list / active /
// sub-agent queries — only the WHERE / ORDER clauses differ. Centralising it
// in `RUN_SUMMARY_SELECT` keeps the column ordering aligned with the row
// mapper and removes the prior copy-pasted SELECT blocks.

const RUN_SUMMARY_SELECT: &str =
    "SELECT r.id, r.task_id, t.name as task_name, r.schedule_id,
            r.agent_id, a.name as agent_name,
            r.state, r.trigger, r.exit_code,
            r.started_at, r.finished_at, r.duration_ms, r.retry_count, r.is_sub_agent,
            r.created_at,
            json_extract(r.metadata, '$.chat_session_id') as chat_session_id,
            r.project_id
     FROM runs r
     LEFT JOIN tasks t ON t.id = r.task_id
     LEFT JOIN agents a ON a.id = r.agent_id";

fn map_run_summary_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunSummary> {
    Ok(RunSummary {
        id: row.get(0)?,
        task_id: row.get(1)?,
        task_name: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        schedule_id: row.get(3)?,
        agent_id: row.get(4)?,
        agent_name: row.get(5)?,
        state: row.get(6)?,
        trigger: row.get(7)?,
        exit_code: row.get(8)?,
        started_at: row.get(9)?,
        finished_at: row.get(10)?,
        duration_ms: row.get(11)?,
        retry_count: row.get(12)?,
        is_sub_agent: row.get::<_, i64>(13)? != 0,
        created_at: row.get(14)?,
        chat_session_id: row.get(15)?,
        // The two queries that don't have project_id in their SELECT use the
        // 17-column shape; this is fine because the index is read positionally.
        project_id: row.get::<_, Option<String>>(16).ok().flatten(),
    })
}

#[async_trait]
impl RunRepo for SqliteRepos {
    async fn list(&self, filter: RunListFilter) -> Result<Vec<RunSummary>, String> {
        self.with_conn(move |conn| {
            // Limit / offset are bound positionally as ?1 / ?2 so the optional
            // filters can append after them without renumbering.
            let mut bound: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            bound.push(Box::new(filter.limit.unwrap_or(100)));
            bound.push(Box::new(filter.offset.unwrap_or(0)));

            let mut sql = format!("{} WHERE 1=1", RUN_SUMMARY_SELECT);

            // Helper that appends a `AND <col> = ?N` clause and pushes the
            // value, keeping the SQL/params in lockstep.
            let mut push_eq = |col: &str, val: Box<dyn rusqlite::ToSql>, sql: &mut String, b: &mut Vec<Box<dyn rusqlite::ToSql>>| {
                let n = b.len() + 1;
                sql.push_str(&format!(" AND {col} = ?{n}"));
                b.push(val);
            };

            if let Some(tid) = filter.task_id {
                push_eq("r.task_id", Box::new(tid), &mut sql, &mut bound);
            }
            if let Some(state) = filter.state_filter {
                if state != "all" {
                    push_eq("r.state", Box::new(state), &mut sql, &mut bound);
                }
            }
            if let Some(pid) = filter.project_id {
                push_eq("r.project_id", Box::new(pid), &mut sql, &mut bound);
            }

            sql.push_str(" ORDER BY r.created_at DESC LIMIT ?1 OFFSET ?2");

            let mut stmt = conn.prepare(&sql).err_str()?;
            let refs: Vec<&dyn rusqlite::ToSql> = bound.iter().map(|p| p.as_ref()).collect();
            let rows: Vec<RunSummary> = stmt
                .query_map(refs.as_slice(), map_run_summary_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn get(&self, id: &str) -> Result<Option<Run>, String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            conn.query_row(
                "SELECT id, task_id, schedule_id, agent_id, state, trigger, exit_code, pid,
                        log_path, started_at, finished_at, duration_ms, retry_count,
                        parent_run_id, metadata, is_sub_agent, created_at, project_id
                 FROM runs WHERE id = ?1",
                params![id],
                |row| {
                    let meta_str: String = row.get(14)?;
                    Ok(Run {
                        id: row.get(0)?,
                        task_id: row.get(1)?,
                        schedule_id: row.get(2)?,
                        agent_id: row.get(3)?,
                        state: row.get(4)?,
                        trigger: row.get(5)?,
                        exit_code: row.get(6)?,
                        pid: row.get(7)?,
                        log_path: row.get(8)?,
                        started_at: row.get(9)?,
                        finished_at: row.get(10)?,
                        duration_ms: row.get(11)?,
                        retry_count: row.get(12)?,
                        parent_run_id: row.get(13)?,
                        metadata: serde_json::from_str(&meta_str)
                            .unwrap_or(serde_json::Value::Null),
                        is_sub_agent: row.get::<_, i64>(15)? != 0,
                        created_at: row.get(16)?,
                        project_id: row.get(17)?,
                    })
                },
            )
            .optional()
            .err_str()
        })
        .await
    }

    async fn list_active(&self) -> Result<Vec<RunSummary>, String> {
        self.with_conn(|conn| {
            let sql = format!(
                "{} WHERE r.state IN ('pending', 'queued', 'running') ORDER BY r.created_at DESC",
                RUN_SUMMARY_SELECT
            );
            let mut stmt = conn.prepare(&sql).err_str()?;
            let rows: Vec<RunSummary> = stmt
                .query_map([], map_run_summary_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn list_sub_agents(&self, parent_run_id: &str) -> Result<Vec<RunSummary>, String> {
        let parent = parent_run_id.to_string();
        self.with_conn(move |conn| {
            let sql = format!(
                "{} WHERE r.parent_run_id = ?1 AND r.is_sub_agent = 1 ORDER BY r.created_at ASC",
                RUN_SUMMARY_SELECT
            );
            let mut stmt = conn.prepare(&sql).err_str()?;
            let rows: Vec<RunSummary> = stmt
                .query_map(params![parent], map_run_summary_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn agent_conversation(
        &self,
        run_id: &str,
    ) -> Result<Option<serde_json::Value>, String> {
        let run_id = run_id.to_string();
        self.with_conn(move |conn| {
            let raw: Option<String> = conn
                .query_row(
                    "SELECT messages FROM agent_conversations WHERE run_id = ?1",
                    params![run_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .err_str()?;
            match raw {
                Some(s) => Ok(Some(serde_json::from_str(&s).err_str()?)),
                None => Ok(None),
            }
        })
        .await
    }

    async fn log_path(&self, run_id: &str) -> Result<Option<String>, String> {
        let run_id = run_id.to_string();
        self.with_conn(move |conn| {
            conn.query_row(
                "SELECT log_path FROM runs WHERE id = ?1",
                params![run_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map(|opt| opt.flatten())
            .err_str()
        })
        .await
    }
}

// ── Work item events ────────────────────────────────────────────────────────

const WORK_ITEM_EVENT_COLUMNS: &str =
    "id, work_item_id, actor_kind, actor_agent_id, kind, payload_json, created_at";

fn map_work_item_event_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkItemEvent> {
    let payload_json: String = row.get(5)?;
    Ok(WorkItemEvent {
        id: row.get(0)?,
        work_item_id: row.get(1)?,
        actor_kind: row.get(2)?,
        actor_agent_id: row.get(3)?,
        kind: row.get(4)?,
        payload: serde_json::from_str(&payload_json).unwrap_or_else(|_| serde_json::json!({})),
        created_at: row.get(6)?,
    })
}

#[async_trait]
impl WorkItemEventRepo for SqliteRepos {
    async fn list(&self, work_item_id: &str) -> Result<Vec<WorkItemEvent>, String> {
        let work_item_id = work_item_id.to_string();
        self.with_conn(move |conn| {
            // Chronological order; ULID id breaks ties for events created in
            // the same second.
            let sql = format!(
                "SELECT {WORK_ITEM_EVENT_COLUMNS} FROM work_item_events
                 WHERE work_item_id = ?1
                 ORDER BY created_at ASC, id ASC"
            );
            let mut stmt = conn.prepare(&sql).err_str()?;
            let rows: Vec<WorkItemEvent> = stmt
                .query_map(params![work_item_id], map_work_item_event_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }
}

// ── Workflow runs ───────────────────────────────────────────────────────────
//
// Thin wrapper: the heavy lifting still lives in `workflows::orchestrator` /
// `workflows::store` because cancel-with-side-effects needs the orchestrator's
// run-loop hooks. This impl just hides the `DbPool` from command code so the
// trait surface stays consistent.

#[async_trait]
impl WorkflowRunRepo for SqliteRepos {
    async fn list_for_workflow(
        &self,
        workflow_id: &str,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>, String> {
        let pool = DbPool(self.pool.0.clone());
        let workflow_id = workflow_id.to_string();
        let limit = limit.clamp(1, 200);
        tokio::task::spawn_blocking(move || {
            crate::workflows::orchestrator::list_runs_for_workflow(&pool, &workflow_id, limit)
        })
        .await
        .err_str()?
    }

    async fn list_for_project(
        &self,
        project_id: &str,
        limit: i64,
    ) -> Result<Vec<WorkflowRunSummary>, String> {
        let pool = DbPool(self.pool.0.clone());
        let project_id = project_id.to_string();
        let limit = limit.clamp(1, 200);
        tokio::task::spawn_blocking(move || {
            crate::workflows::store::list_runs_for_project(&pool, &project_id, limit)
        })
        .await
        .err_str()?
    }

    async fn get_with_steps(&self, run_id: &str) -> Result<WorkflowRunWithSteps, String> {
        let pool = DbPool(self.pool.0.clone());
        let run_id = run_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<WorkflowRunWithSteps, String> {
            let (run, steps) = crate::workflows::orchestrator::load_run_with_steps(&pool, &run_id)?;
            Ok(WorkflowRunWithSteps { run, steps })
        })
        .await
        .err_str()?
    }

    async fn cancel(&self, run_id: &str) -> Result<(), String> {
        let pool = DbPool(self.pool.0.clone());
        let run_id = run_id.to_string();
        tokio::task::spawn_blocking(move || {
            crate::workflows::orchestrator::cancel_run(&pool, &run_id)
        })
        .await
        .err_str()?
    }
}

// ── Project boards ──────────────────────────────────────────────────────────

const PROJECT_BOARD_COLUMNS: &str =
    "id, project_id, name, prefix, position, is_default, created_at, updated_at";

fn map_project_board_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectBoard> {
    Ok(ProjectBoard {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        prefix: row.get(3)?,
        position: row.get(4)?,
        is_default: row.get::<_, bool>(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn validate_board_prefix(prefix: &str) -> Result<(), String> {
    let trimmed = prefix.trim();
    if trimmed.len() < 2 || trimmed.len() > 8 {
        return Err("board prefix must be 2 to 8 characters long".into());
    }
    if !trimmed.chars().all(|c| c.is_ascii_uppercase()) {
        return Err("board prefix must contain only uppercase letters A–Z".into());
    }
    Ok(())
}

#[async_trait]
impl ProjectBoardRepo for SqliteRepos {
    async fn list(&self, project_id: &str) -> Result<Vec<ProjectBoard>, String> {
        let project_id = project_id.to_string();
        self.with_conn(move |conn| {
            // Default board first, then explicit position, then creation order.
            let sql = format!(
                "SELECT {PROJECT_BOARD_COLUMNS} FROM project_boards \
                 WHERE project_id = ?1 \
                 ORDER BY is_default DESC, position ASC, created_at ASC"
            );
            let mut stmt = conn.prepare(&sql).err_str()?;
            let rows: Vec<ProjectBoard> = stmt
                .query_map(params![project_id], map_project_board_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn get(&self, id: &str) -> Result<Option<ProjectBoard>, String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            conn.query_row(
                &format!("SELECT {PROJECT_BOARD_COLUMNS} FROM project_boards WHERE id = ?1"),
                params![id],
                map_project_board_row,
            )
            .optional()
            .err_str()
        })
        .await
    }

    async fn create(&self, payload: CreateProjectBoard) -> Result<ProjectBoard, String> {
        validate_board_prefix(&payload.prefix)?;
        self.with_conn(move |conn| {
            let name = payload.name.trim();
            if name.is_empty() {
                return Err("board name must be non-empty".into());
            }
            let prefix = payload.prefix.trim().to_string();

            // Prefix uniqueness is per-project, mirroring board names in
            // tools like Linear/Jira.
            let prefix_taken: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM project_boards WHERE project_id = ?1 AND prefix = ?2)",
                    params![payload.project_id, prefix],
                    |row| row.get(0),
                )
                .err_str()?;
            if prefix_taken {
                return Err(format!(
                    "a board with prefix '{}' already exists in this project",
                    prefix
                ));
            }

            let now = chrono::Utc::now().to_rfc3339();
            let id = Ulid::new().to_string();

            // Floating-point position lets us insert between siblings without
            // renumbering everyone — large step gives plenty of headroom.
            let next_position: f64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(position), 0) FROM project_boards WHERE project_id = ?1",
                    params![payload.project_id],
                    |row| row.get(0),
                )
                .err_str()?;
            let position = next_position + 1024.0;

            conn.execute(
                "INSERT INTO project_boards (id, project_id, name, prefix, position, is_default, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?6)",
                params![id, payload.project_id, name, prefix, position, now],
            )
            .err_str()?;

            conn.query_row(
                &format!("SELECT {PROJECT_BOARD_COLUMNS} FROM project_boards WHERE id = ?1"),
                params![id],
                map_project_board_row,
            )
            .err_str()
        })
        .await
    }

    async fn update(
        &self,
        id: &str,
        payload: UpdateProjectBoard,
    ) -> Result<ProjectBoard, String> {
        if let Some(prefix) = payload.prefix.as_deref() {
            validate_board_prefix(prefix)?;
        }
        let id = id.to_string();
        self.with_conn(move |conn| {
            let existing: ProjectBoard = conn
                .query_row(
                    &format!("SELECT {PROJECT_BOARD_COLUMNS} FROM project_boards WHERE id = ?1"),
                    params![id],
                    map_project_board_row,
                )
                .optional()
                .err_str()?
                .ok_or_else(|| format!("board '{}' not found", id))?;
            let now = chrono::Utc::now().to_rfc3339();

            if let Some(name) = payload.name.as_deref() {
                let name = name.trim();
                if name.is_empty() {
                    return Err("board name must be non-empty".into());
                }
                conn.execute(
                    "UPDATE project_boards SET name = ?1, updated_at = ?2 WHERE id = ?3",
                    params![name, now, id],
                )
                .err_str()?;
            }
            if let Some(prefix) = payload.prefix.as_deref() {
                let prefix = prefix.trim().to_string();
                if prefix != existing.prefix {
                    let taken: bool = conn
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM project_boards WHERE project_id = ?1 AND prefix = ?2 AND id != ?3)",
                            params![existing.project_id, prefix, id],
                            |row| row.get(0),
                        )
                        .err_str()?;
                    if taken {
                        return Err(format!(
                            "a board with prefix '{}' already exists in this project",
                            prefix
                        ));
                    }
                    conn.execute(
                        "UPDATE project_boards SET prefix = ?1, updated_at = ?2 WHERE id = ?3",
                        params![prefix, now, id],
                    )
                    .err_str()?;
                }
            }

            conn.query_row(
                &format!("SELECT {PROJECT_BOARD_COLUMNS} FROM project_boards WHERE id = ?1"),
                params![id],
                map_project_board_row,
            )
            .err_str()
        })
        .await
    }

    async fn delete(&self, id: &str, payload: DeleteProjectBoard) -> Result<(), String> {
        let id = id.to_string();
        self.with_conn_mut(move |conn| {
            // Re-fetch existing inside the same transaction so we can rely on
            // the project_id / is_default state being consistent.
            let existing: ProjectBoard = conn
                .query_row(
                    &format!("SELECT {PROJECT_BOARD_COLUMNS} FROM project_boards WHERE id = ?1"),
                    params![id],
                    map_project_board_row,
                )
                .optional()
                .err_str()?
                .ok_or_else(|| format!("board '{}' not found", id))?;

            // We can't delete the only board — every project must have ≥1.
            let siblings: Vec<ProjectBoard> = {
                let sql = format!(
                    "SELECT {PROJECT_BOARD_COLUMNS} FROM project_boards \
                     WHERE project_id = ?1 \
                     ORDER BY is_default DESC, position ASC, created_at ASC"
                );
                let mut stmt = conn.prepare(&sql).err_str()?;
                let rows: Vec<ProjectBoard> = stmt
                    .query_map(params![existing.project_id], map_project_board_row)
                    .err_str()?
                    .filter_map(|r| r.ok())
                    .collect();
                rows
            };
            if siblings.len() <= 1 {
                return Err("cannot delete the last remaining board".into());
            }

            // If this board has work items, the caller must either pick a
            // destination board to re-parent them into, or pass `force = true`
            // (which deletes everything via FK cascade).
            let item_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM work_items WHERE board_id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .err_str()?;

            let destination = match payload.destination_board_id.as_deref() {
                Some(dest_id) => {
                    let dest: ProjectBoard = conn
                        .query_row(
                            &format!(
                                "SELECT {PROJECT_BOARD_COLUMNS} FROM project_boards WHERE id = ?1"
                            ),
                            params![dest_id],
                            map_project_board_row,
                        )
                        .optional()
                        .err_str()?
                        .ok_or_else(|| format!("destination board '{}' not found", dest_id))?;
                    if dest.project_id != existing.project_id {
                        return Err(
                            "destination board belongs to a different project".into(),
                        );
                    }
                    if dest.id == existing.id {
                        return Err(
                            "destination board must be different from the board being deleted"
                                .into(),
                        );
                    }
                    Some(dest)
                }
                None => None,
            };

            if item_count > 0 && destination.is_none() && !payload.force.unwrap_or(false) {
                return Err(
                    "choose a destination board before deleting a board that has items".into(),
                );
            }

            let now = chrono::Utc::now().to_rfc3339();
            let tx = conn.transaction().err_str()?;

            if let Some(destination) = destination.as_ref() {
                // Re-parent every column and work item to the destination.
                tx.execute(
                    "UPDATE project_board_columns SET board_id = ?1, updated_at = ?2 WHERE board_id = ?3",
                    params![destination.id, now, id],
                )
                .err_str()?;
                tx.execute(
                    "UPDATE work_items SET board_id = ?1, updated_at = ?2 WHERE board_id = ?3",
                    params![destination.id, now, id],
                )
                .err_str()?;
            }

            // If we're deleting the default board, promote a sibling first so
            // the partial unique index on (project_id, is_default) stays valid.
            if existing.is_default {
                let next_default = siblings
                    .iter()
                    .find(|b| b.id != id)
                    .ok_or_else(|| "expected at least one remaining board".to_string())?;
                tx.execute(
                    "UPDATE project_boards SET is_default = 0, updated_at = ?1 WHERE id = ?2",
                    params![now, id],
                )
                .err_str()?;
                tx.execute(
                    "UPDATE project_boards SET is_default = 1, updated_at = ?1 WHERE id = ?2",
                    params![now, next_default.id],
                )
                .err_str()?;
            }

            tx.execute("DELETE FROM project_boards WHERE id = ?1", params![id])
                .err_str()?;
            tx.commit().err_str()?;
            Ok(())
        })
        .await
    }
}

// ── Bus messages ────────────────────────────────────────────────────────────

const BUS_MESSAGE_COLUMNS: &str =
    "id, from_agent_id, from_run_id, from_session_id, to_agent_id, to_run_id, to_session_id, \
     kind, event_type, payload, status, created_at";

fn map_bus_message_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BusMessage> {
    let payload_str: String = row.get(9)?;
    Ok(BusMessage {
        id: row.get(0)?,
        from_agent_id: row.get(1)?,
        from_run_id: row.get(2)?,
        from_session_id: row.get(3)?,
        to_agent_id: row.get(4)?,
        to_run_id: row.get(5)?,
        to_session_id: row.get(6)?,
        kind: row.get(7)?,
        event_type: row.get(8)?,
        payload: serde_json::from_str(&payload_str).unwrap_or(serde_json::Value::Null),
        status: row.get(10)?,
        created_at: row.get(11)?,
    })
}

#[async_trait]
impl BusMessageRepo for SqliteRepos {
    async fn list(
        &self,
        agent_id: Option<String>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<BusMessage>, String> {
        self.with_conn(move |conn| {
            // Two SQL shapes: filtered by agent (sender or recipient) vs. all.
            // Building both with the same column projection so the row mapper
            // doesn't need to vary.
            let (sql, bound): (String, Vec<Box<dyn rusqlite::ToSql>>) = match agent_id {
                Some(aid) => (
                    format!(
                        "SELECT {BUS_MESSAGE_COLUMNS} FROM bus_messages \
                         WHERE from_agent_id = ?1 OR to_agent_id = ?1 \
                         ORDER BY created_at DESC LIMIT ?2 OFFSET ?3"
                    ),
                    vec![Box::new(aid), Box::new(limit), Box::new(offset)],
                ),
                None => (
                    format!(
                        "SELECT {BUS_MESSAGE_COLUMNS} FROM bus_messages \
                         ORDER BY created_at DESC LIMIT ?1 OFFSET ?2"
                    ),
                    vec![Box::new(limit), Box::new(offset)],
                ),
            };
            let mut stmt = conn.prepare(&sql).err_str()?;
            let refs: Vec<&dyn rusqlite::ToSql> = bound.iter().map(|p| p.as_ref()).collect();
            let rows: Vec<BusMessage> = stmt
                .query_map(refs.as_slice(), map_bus_message_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn thread_for_agent(
        &self,
        agent_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<PaginatedBusThread, String> {
        let agent_id = agent_id.to_string();
        self.with_conn(move |conn| {
            let total_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM bus_messages WHERE to_agent_id = ?1",
                    params![agent_id],
                    |row| row.get(0),
                )
                .err_str()?;

            // Joined view: each message is annotated with the run/session it
            // triggered, so the inbox UI can render status without N+1 calls.
            let mut stmt = conn
                .prepare(
                    "SELECT bm.id, bm.from_agent_id, COALESCE(a.name, bm.from_agent_id), bm.to_agent_id, bm.kind,
                            bm.payload, bm.status, bm.created_at,
                            bm.to_run_id, r.state, json_extract(r.metadata, '$.finish_summary'),
                            bm.to_session_id, cs.execution_state, cs.finish_summary
                     FROM bus_messages bm
                     LEFT JOIN agents a ON a.id = bm.from_agent_id
                     LEFT JOIN runs r ON r.id = bm.to_run_id
                     LEFT JOIN chat_sessions cs ON cs.id = bm.to_session_id
                     WHERE bm.to_agent_id = ?1
                     ORDER BY bm.created_at DESC
                     LIMIT ?2 OFFSET ?3",
                )
                .err_str()?;

            let messages: Vec<BusThreadMessage> = stmt
                .query_map(params![agent_id, limit, offset], |row| {
                    let payload_str: String = row.get(5)?;
                    Ok(BusThreadMessage {
                        id: row.get(0)?,
                        from_agent_id: row.get(1)?,
                        from_agent_name: row.get(2)?,
                        to_agent_id: row.get(3)?,
                        kind: row.get(4)?,
                        // Bus payload may be stringly-typed (legacy senders) — fall
                        // back to wrapping it in a JSON string if parsing fails.
                        payload: serde_json::from_str(&payload_str)
                            .unwrap_or_else(|_| serde_json::Value::String(payload_str.clone())),
                        status: row.get(6)?,
                        created_at: row.get(7)?,
                        triggered_run_id: row.get(8)?,
                        triggered_run_state: row.get(9)?,
                        triggered_run_summary: row.get(10)?,
                        triggered_session_id: row.get(11)?,
                        triggered_session_state: row.get(12)?,
                        triggered_session_summary: row.get(13)?,
                    })
                })
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();

            let has_more = (offset + limit) < total_count;
            Ok(PaginatedBusThread {
                messages,
                total_count,
                has_more,
            })
        })
        .await
    }
}

// ── Bus subscriptions ───────────────────────────────────────────────────────

const BUS_SUBSCRIPTION_COLUMNS: &str =
    "id, subscriber_agent_id, source_agent_id, event_type, task_id, payload_template, \
     enabled, max_chain_depth, created_at, updated_at";

fn map_bus_subscription_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BusSubscription> {
    Ok(BusSubscription {
        id: row.get(0)?,
        subscriber_agent_id: row.get(1)?,
        source_agent_id: row.get(2)?,
        event_type: row.get(3)?,
        task_id: row.get(4)?,
        payload_template: row.get(5)?,
        enabled: row.get(6)?,
        max_chain_depth: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

#[async_trait]
impl BusSubscriptionRepo for SqliteRepos {
    async fn list(&self, agent_id: Option<String>) -> Result<Vec<BusSubscription>, String> {
        self.with_conn(move |conn| {
            let (sql, bound): (String, Vec<Box<dyn rusqlite::ToSql>>) = match agent_id {
                Some(aid) => (
                    format!(
                        "SELECT {BUS_SUBSCRIPTION_COLUMNS} FROM bus_subscriptions \
                         WHERE subscriber_agent_id = ?1 OR source_agent_id = ?1 \
                         ORDER BY created_at DESC"
                    ),
                    vec![Box::new(aid)],
                ),
                None => (
                    format!(
                        "SELECT {BUS_SUBSCRIPTION_COLUMNS} FROM bus_subscriptions \
                         ORDER BY created_at DESC"
                    ),
                    vec![],
                ),
            };
            let mut stmt = conn.prepare(&sql).err_str()?;
            let refs: Vec<&dyn rusqlite::ToSql> = bound.iter().map(|p| p.as_ref()).collect();
            let rows: Vec<BusSubscription> = stmt
                .query_map(refs.as_slice(), map_bus_subscription_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn create(&self, payload: CreateBusSubscription) -> Result<BusSubscription, String> {
        self.with_conn(move |conn| {
            let id = Ulid::new().to_string();
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO bus_subscriptions (id, subscriber_agent_id, source_agent_id, event_type, task_id, payload_template, enabled, max_chain_depth, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?8)",
                params![
                    id,
                    payload.subscriber_agent_id,
                    payload.source_agent_id,
                    payload.event_type,
                    payload.task_id,
                    payload.payload_template,
                    payload.max_chain_depth,
                    now,
                ],
            )
            .err_str()?;

            Ok(BusSubscription {
                id,
                subscriber_agent_id: payload.subscriber_agent_id,
                source_agent_id: payload.source_agent_id,
                event_type: payload.event_type,
                task_id: payload.task_id,
                payload_template: payload.payload_template,
                enabled: true,
                max_chain_depth: payload.max_chain_depth,
                created_at: now.clone(),
                updated_at: now,
            })
        })
        .await
    }

    async fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE bus_subscriptions SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
                params![enabled, now, id],
            )
            .err_str()?;
            Ok(())
        })
        .await
    }

    async fn delete(&self, id: &str) -> Result<(), String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            conn.execute("DELETE FROM bus_subscriptions WHERE id = ?1", params![id])
                .err_str()?;
            Ok(())
        })
        .await
    }
}

// ── Chat ────────────────────────────────────────────────────────────────────
//
// Read-only at the trait surface. Sessions are joined with bus_messages /
// agents / parent chat_sessions so the inbox UI can render "from agent X via
// session Y" subtitles in one round-trip.

const CHAT_SESSION_SELECT: &str = "SELECT cs.id, cs.agent_id, cs.title, cs.archived, cs.session_type, cs.parent_session_id, cs.source_bus_message_id,
                cs.chain_depth, cs.execution_state, cs.finish_summary, cs.terminal_error,
                bm.from_agent_id, a.name,
                src.id, src.title,
                cs.created_at, cs.updated_at, cs.project_id,
                cs.worktree_name, cs.worktree_branch, cs.worktree_path
         FROM chat_sessions cs
         LEFT JOIN bus_messages bm ON bm.id = cs.source_bus_message_id
         LEFT JOIN agents a ON a.id = bm.from_agent_id
         LEFT JOIN chat_sessions src ON src.id = bm.from_session_id";

fn map_chat_session_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChatSession> {
    Ok(ChatSession {
        id: row.get(0)?,
        agent_id: row.get(1)?,
        title: row.get(2)?,
        archived: row.get::<_, bool>(3)?,
        session_type: row.get(4)?,
        parent_session_id: row.get(5)?,
        source_bus_message_id: row.get(6)?,
        chain_depth: row.get(7)?,
        execution_state: row.get(8)?,
        finish_summary: row.get(9)?,
        terminal_error: row.get(10)?,
        source_agent_id: row.get(11)?,
        source_agent_name: row.get(12)?,
        source_session_id: row.get(13)?,
        source_session_title: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
        project_id: row.get(17)?,
        worktree_name: row.get(18)?,
        worktree_branch: row.get(19)?,
        worktree_path: row.get(20)?,
    })
}

fn map_chat_message_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChatMessageRow> {
    Ok(ChatMessageRow {
        id: row.get(0)?,
        role: row.get(1)?,
        content_json: row.get(2)?,
        created_at: row.get(3)?,
        is_compacted: row.get::<_, bool>(4)?,
    })
}

#[async_trait]
impl ChatRepo for SqliteRepos {
    async fn list_sessions(
        &self,
        filter: ChatSessionListFilter,
    ) -> Result<Vec<ChatSession>, String> {
        self.with_conn(move |conn| {
            // Filters compose dynamically: agent_id is mandatory (?1),
            // optional project_id pinning + optional session_type IN-list
            // append after with sequential placeholders.
            let mut sql = format!("{CHAT_SESSION_SELECT} WHERE cs.agent_id = ?1");
            let mut bound: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(filter.agent_id)];

            if !filter.include_archived {
                sql.push_str(" AND cs.archived = 0");
            }
            if let Some(pid) = filter.project_id {
                let n = bound.len() + 1;
                sql.push_str(&format!(" AND cs.project_id = ?{n}"));
                bound.push(Box::new(pid));
            }
            if !filter.session_types.is_empty() {
                let start = bound.len() + 1;
                let placeholders = (0..filter.session_types.len())
                    .map(|i| format!("?{}", start + i))
                    .collect::<Vec<_>>()
                    .join(", ");
                sql.push_str(&format!(" AND cs.session_type IN ({placeholders})"));
                for t in filter.session_types {
                    bound.push(Box::new(t));
                }
            }
            sql.push_str(" ORDER BY cs.updated_at DESC");

            let mut stmt = conn.prepare(&sql).err_str()?;
            let refs: Vec<&dyn rusqlite::ToSql> = bound.iter().map(|p| p.as_ref()).collect();
            let rows: Vec<ChatSession> = stmt
                .query_map(refs.as_slice(), map_chat_session_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn get_messages(
        &self,
        session_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<ChatMessageRows, String> {
        let session_id = session_id.to_string();
        self.with_conn(move |conn| {
            let total_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM chat_messages WHERE session_id = ?1",
                    params![session_id],
                    |row| row.get(0),
                )
                .err_str()?;

            // Two SQL shapes: paginated (limit > 0) reverses then re-orders so
            // the latest N appear ascending; unpaginated streams the whole
            // session in chronological order.
            let messages: Vec<ChatMessageRow> = if limit > 0 {
                let mut stmt = conn
                    .prepare(
                        "SELECT id, role, content, created_at, is_compacted FROM (
                           SELECT id, role, content, created_at, is_compacted
                           FROM chat_messages WHERE session_id = ?1
                           ORDER BY created_at DESC
                           LIMIT ?2 OFFSET ?3
                         ) sub ORDER BY created_at ASC",
                    )
                    .err_str()?;
                let rows: Vec<ChatMessageRow> = stmt
                    .query_map(params![session_id, limit, offset], map_chat_message_row)
                    .err_str()?
                    .filter_map(|r| r.ok())
                    .collect();
                rows
            } else {
                let mut stmt = conn
                    .prepare(
                        "SELECT id, role, content, created_at, is_compacted FROM chat_messages
                         WHERE session_id = ?1 ORDER BY created_at ASC",
                    )
                    .err_str()?;
                let rows: Vec<ChatMessageRow> = stmt
                    .query_map(params![session_id], map_chat_message_row)
                    .err_str()?
                    .filter_map(|r| r.ok())
                    .collect();
                rows
            };

            let has_more = limit > 0 && (offset + limit) < total_count;
            Ok(ChatMessageRows {
                messages,
                total_count,
                has_more,
            })
        })
        .await
    }

    async fn session_meta(&self, session_id: &str) -> Result<ChatSessionMeta, String> {
        let session_id = session_id.to_string();
        self.with_conn(move |conn| {
            let sid = session_id.clone();
            conn.query_row(
                "SELECT cs.agent_id, cs.project_id, p.name
                 FROM chat_sessions cs
                 LEFT JOIN projects p ON p.id = cs.project_id
                 WHERE cs.id = ?1",
                params![session_id],
                |row| {
                    Ok(ChatSessionMeta {
                        session_id: sid.clone(),
                        agent_id: row.get(0)?,
                        project_id: row.get(1)?,
                        project_name: row.get(2)?,
                    })
                },
            )
            .err_str()
        })
        .await
    }

    async fn session_execution(
        &self,
        session_id: &str,
    ) -> Result<SessionExecutionStatus, String> {
        let session_id = session_id.to_string();
        self.with_conn(move |conn| {
            let sid = session_id.clone();
            conn.query_row(
                "SELECT execution_state, finish_summary, terminal_error \
                 FROM chat_sessions WHERE id = ?1",
                params![session_id],
                |row| {
                    Ok(SessionExecutionStatus {
                        session_id: sid.clone(),
                        execution_state: row.get(0)?,
                        finish_summary: row.get(1)?,
                        terminal_error: row.get(2)?,
                    })
                },
            )
            .err_str()
        })
        .await
    }

    async fn session_type(&self, session_id: &str) -> Result<String, String> {
        let session_id = session_id.to_string();
        self.with_conn(move |conn| {
            conn.query_row(
                "SELECT session_type FROM chat_sessions WHERE id = ?1",
                params![session_id],
                |row| row.get::<_, String>(0),
            )
            .err_str()
        })
        .await
    }

    async fn list_message_reactions(
        &self,
        session_id: &str,
    ) -> Result<Vec<MessageReactionRow>, String> {
        let session_id = session_id.to_string();
        self.with_conn(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, message_id, emoji, created_at FROM message_reactions \
                     WHERE session_id = ?1 ORDER BY created_at ASC",
                )
                .err_str()?;
            let rows: Vec<MessageReactionRow> = stmt
                .query_map(params![session_id], |row| {
                    Ok(MessageReactionRow {
                        id: row.get(0)?,
                        message_id: row.get(1)?,
                        emoji: row.get(2)?,
                        created_at: row.get(3)?,
                    })
                })
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn token_usage(&self, session_id: &str) -> Result<ChatSessionTokenUsage, String> {
        let session_id = session_id.to_string();
        self.with_conn(move |conn| {
            conn.query_row(
                "SELECT last_input_tokens, agent_id FROM chat_sessions WHERE id = ?1",
                params![session_id],
                |row| {
                    Ok(ChatSessionTokenUsage {
                        last_input_tokens: row.get(0)?,
                        agent_id: row.get(1)?,
                    })
                },
            )
            .map_err(|e| format!("session not found: {}", e))
        })
        .await
    }
}

// ── Work items ──────────────────────────────────────────────────────────────
//
// Read-only at the trait surface — see `commands/work_items.rs` for the
// `*_with_db` write helpers that still drive cross-table event inserts.

const WORK_ITEM_REPO_COLUMNS: &str =
    "id, project_id, board_id, title, description, kind, column_id, status, priority,
     assignee_agent_id, created_by_agent_id, parent_work_item_id, position,
     labels, metadata, blocked_reason, started_at, completed_at, created_at, updated_at";

fn map_work_item_repo_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkItem> {
    let labels_json: String = row.get(13)?;
    let metadata_json: String = row.get(14)?;
    Ok(WorkItem {
        id: row.get(0)?,
        project_id: row.get(1)?,
        board_id: row.get(2)?,
        title: row.get(3)?,
        description: row.get(4)?,
        kind: row.get(5)?,
        column_id: row.get(6)?,
        status: row.get(7)?,
        priority: row.get(8)?,
        assignee_agent_id: row.get(9)?,
        created_by_agent_id: row.get(10)?,
        parent_work_item_id: row.get(11)?,
        position: row.get(12)?,
        labels: serde_json::from_str(&labels_json).unwrap_or_default(),
        metadata: serde_json::from_str(&metadata_json).unwrap_or_else(|_| serde_json::json!({})),
        blocked_reason: row.get(15)?,
        started_at: row.get(16)?,
        completed_at: row.get(17)?,
        created_at: row.get(18)?,
        updated_at: row.get(19)?,
    })
}

const WORK_ITEM_COMMENT_REPO_COLUMNS: &str =
    "id, work_item_id, author_kind, author_agent_id, body, created_at, updated_at";

fn map_work_item_comment_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkItemComment> {
    Ok(WorkItemComment {
        id: row.get(0)?,
        work_item_id: row.get(1)?,
        author_kind: row.get(2)?,
        author_agent_id: row.get(3)?,
        body: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

#[async_trait]
impl WorkItemRepo for SqliteRepos {
    async fn list(
        &self,
        project_id: &str,
        board_id: Option<String>,
    ) -> Result<Vec<WorkItem>, String> {
        let project_id = project_id.to_string();
        self.with_conn(move |conn| {
            // Order by column-or-status grouping then by board position so the
            // kanban view's lane-by-lane render is just iteration over the
            // result set.
            let (sql, bound): (String, Vec<Box<dyn rusqlite::ToSql>>) = match board_id {
                Some(b) => (
                    format!(
                        "SELECT {WORK_ITEM_REPO_COLUMNS} FROM work_items \
                         WHERE project_id = ?1 AND board_id = ?2 \
                         ORDER BY COALESCE(column_id, status), position ASC"
                    ),
                    vec![Box::new(project_id), Box::new(b)],
                ),
                None => (
                    format!(
                        "SELECT {WORK_ITEM_REPO_COLUMNS} FROM work_items \
                         WHERE project_id = ?1 \
                         ORDER BY COALESCE(column_id, status), position ASC"
                    ),
                    vec![Box::new(project_id)],
                ),
            };
            let mut stmt = conn.prepare(&sql).err_str()?;
            let refs: Vec<&dyn rusqlite::ToSql> = bound.iter().map(|p| p.as_ref()).collect();
            let rows: Vec<WorkItem> = stmt
                .query_map(refs.as_slice(), map_work_item_repo_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn get(&self, id: &str) -> Result<WorkItem, String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            conn.query_row(
                &format!("SELECT {WORK_ITEM_REPO_COLUMNS} FROM work_items WHERE id = ?1"),
                params![id],
                map_work_item_repo_row,
            )
            .err_str()
        })
        .await
    }

    async fn list_comments(&self, work_item_id: &str) -> Result<Vec<WorkItemComment>, String> {
        let work_item_id = work_item_id.to_string();
        self.with_conn(move |conn| {
            let sql = format!(
                "SELECT {WORK_ITEM_COMMENT_REPO_COLUMNS} FROM work_item_comments \
                 WHERE work_item_id = ?1 ORDER BY created_at ASC"
            );
            let mut stmt = conn.prepare(&sql).err_str()?;
            let rows: Vec<WorkItemComment> = stmt
                .query_map(params![work_item_id], map_work_item_comment_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }
}

// ── Project workflows ───────────────────────────────────────────────────────
//
// Read-only at the trait surface. Writes (create / update / delete /
// set-enabled) go through `*_with_db` helpers because they involve graph
// normalisation, trigger reconciliation, and a transactional graph swap.

const PROJECT_WORKFLOW_COLUMNS: &str = "id, project_id, name, description, enabled, graph,
        trigger_kind, trigger_config, version, created_at, updated_at";

fn map_project_workflow_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectWorkflow> {
    let enabled: i64 = row.get(4)?;
    let graph_json: String = row.get(5)?;
    let trigger_config_json: String = row.get(7)?;
    Ok(ProjectWorkflow {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        enabled: enabled != 0,
        graph: serde_json::from_str(&graph_json).unwrap_or_default(),
        trigger_kind: row.get(6)?,
        trigger_config: serde_json::from_str(&trigger_config_json)
            .unwrap_or_else(|_| serde_json::json!({})),
        version: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

#[async_trait]
impl ProjectWorkflowRepo for SqliteRepos {
    async fn list(
        &self,
        project_id: &str,
        limit: i64,
    ) -> Result<Vec<ProjectWorkflow>, String> {
        let project_id = project_id.to_string();
        let limit = limit.clamp(1, 200);
        self.with_conn(move |conn| {
            let sql = format!(
                "SELECT {PROJECT_WORKFLOW_COLUMNS} FROM project_workflows \
                 WHERE project_id = ?1 ORDER BY name ASC LIMIT ?2"
            );
            let mut stmt = conn.prepare(&sql).err_str()?;
            let rows: Vec<ProjectWorkflow> = stmt
                .query_map(params![project_id, limit], map_project_workflow_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn get(&self, id: &str) -> Result<ProjectWorkflow, String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            conn.query_row(
                &format!(
                    "SELECT {PROJECT_WORKFLOW_COLUMNS} FROM project_workflows WHERE id = ?1"
                ),
                params![id],
                map_project_workflow_row,
            )
            .err_str()
        })
        .await
    }

    async fn lookup_project_id(&self, workflow_id: &str) -> Result<String, String> {
        let workflow_id = workflow_id.to_string();
        self.with_conn(move |conn| {
            conn.query_row(
                "SELECT project_id FROM project_workflows WHERE id = ?1",
                params![workflow_id],
                |row| row.get::<_, String>(0),
            )
            .map_err(|e| format!("workflow: not found ({})", e))
        })
        .await
    }

    async fn lookup_run_scope(&self, run_id: &str) -> Result<(String, String), String> {
        let run_id = run_id.to_string();
        self.with_conn(move |conn| {
            // Joined on `project_workflows.id = workflow_runs.workflow_id` so
            // the dispatcher can scope events to the owning project without
            // a follow-up query.
            conn.query_row(
                "SELECT wr.workflow_id, pw.project_id
                 FROM workflow_runs wr
                 INNER JOIN project_workflows pw ON pw.id = wr.workflow_id
                 WHERE wr.id = ?1",
                params![run_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(|e| format!("workflow run not found ({})", e))
        })
        .await
    }
}

// ── Project board columns ──────────────────────────────────────────────────
//
// Read-only at the trait surface. Writes (create / update / delete with
// re-parent) stay in `commands/project_board_columns.rs` because they
// involve default-column promotion, optimistic-concurrency `expected_revision`
// checks, and cross-table re-parenting.

const PROJECT_BOARD_COLUMN_COLUMNS: &str =
    "id, project_id, board_id, name, role, is_default, position, created_at, updated_at";

fn map_project_board_column_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ProjectBoardColumn> {
    Ok(ProjectBoardColumn {
        id: row.get(0)?,
        project_id: row.get(1)?,
        board_id: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        name: row.get(3)?,
        role: row.get(4)?,
        is_default: row.get::<_, bool>(5)?,
        position: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

#[async_trait]
impl ProjectBoardColumnRepo for SqliteRepos {
    async fn list(
        &self,
        project_id: &str,
        board_id: Option<String>,
    ) -> Result<Vec<ProjectBoardColumn>, String> {
        let project_id = project_id.to_string();
        self.with_conn(move |conn| {
            // When no board_id is given, fall back to the project's default
            // board so the kanban surface always has a sensible default
            // viewport even before the user has explicitly picked one.
            let effective_board_id: Option<String> = match board_id {
                Some(b) => Some(b),
                None => conn
                    .query_row(
                        "SELECT id FROM project_boards \
                         WHERE project_id = ?1 AND is_default = 1 LIMIT 1",
                        params![project_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .err_str()?,
            };

            let (sql, bound): (String, Vec<Box<dyn rusqlite::ToSql>>) = match effective_board_id {
                Some(b) => (
                    format!(
                        "SELECT {PROJECT_BOARD_COLUMN_COLUMNS} FROM project_board_columns \
                         WHERE project_id = ?1 AND board_id = ?2 \
                         ORDER BY position ASC, created_at ASC"
                    ),
                    vec![Box::new(project_id), Box::new(b)],
                ),
                None => (
                    format!(
                        "SELECT {PROJECT_BOARD_COLUMN_COLUMNS} FROM project_board_columns \
                         WHERE project_id = ?1 \
                         ORDER BY position ASC, created_at ASC"
                    ),
                    vec![Box::new(project_id)],
                ),
            };
            let mut stmt = conn.prepare(&sql).err_str()?;
            let refs: Vec<&dyn rusqlite::ToSql> = bound.iter().map(|p| p.as_ref()).collect();
            let rows: Vec<ProjectBoardColumn> = stmt
                .query_map(refs.as_slice(), map_project_board_column_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn get(&self, id: &str) -> Result<Option<ProjectBoardColumn>, String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            conn.query_row(
                &format!(
                    "SELECT {PROJECT_BOARD_COLUMN_COLUMNS} FROM project_board_columns WHERE id = ?1"
                ),
                params![id],
                map_project_board_column_row,
            )
            .optional()
            .err_str()
        })
        .await
    }
}
