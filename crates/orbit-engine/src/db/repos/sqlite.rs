//! SQLite-backed `Repos` impl built on the existing rusqlite/r2d2 pool.
//!
//! Until rusqlite is removed, this is what every desktop and per-tenant-VM
//! deployment runs. Queries are lifted verbatim from the original
//! `commands/{tasks,…}.rs` so behaviour is identical — the only architectural
//! change is that they're reachable through the `Repos` trait instead of via
//! `tauri::State<DbPool>`.

use async_trait::async_trait;
use rusqlite::{params, OptionalExtension};
use std::collections::HashSet;
use ulid::Ulid;

use crate::commands::project_board_columns::{
    list_project_board_columns_sync, resolve_board_column_sync,
};
use crate::commands::work_item_events::{event_kind, insert_event, Actor};
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
use crate::models::project::{
    CreateProject, Project, ProjectAgent, ProjectAgentWithMeta, ProjectSummary, UpdateProject,
};
use crate::models::project_board::{
    CreateProjectBoard, DeleteProjectBoard, ProjectBoard, UpdateProjectBoard,
};
use crate::models::project_board_column::ProjectBoardColumn;
use crate::models::project_workflow::{
    CreateProjectWorkflow, ProjectWorkflow, RuleNode, UpdateProjectWorkflow, WorkflowEdge,
    WorkflowGraph, KNOWN_NODE_TYPES, RULE_OPERATORS,
};
use crate::models::run::{Run, RunSummary};
use crate::models::schedule::{CreateSchedule, RecurringConfig, Schedule};
use crate::models::task::{CreateTask, Task, UpdateTask};
use crate::models::user::User;
use crate::models::work_item::{CreateWorkItem, UpdateWorkItem, WorkItem};
use crate::models::work_item_comment::{CommentAuthor, WorkItemComment};
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
            let tags_str = serde_json::to_string(&payload.tags.unwrap_or_default()).err_str()?;
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
                        &format!(
                            "UPDATE tasks SET {} = ?1, updated_at = ?2 WHERE id = ?3",
                            $column
                        ),
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
                        &format!(
                            "UPDATE agents SET {} = ?1, updated_at = ?2 WHERE id = ?3",
                            $column
                        ),
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

    async fn list_agents(&self, project_id: &str) -> Result<Vec<Agent>, String> {
        let project_id = project_id.to_string();
        self.with_conn(move |conn| {
            // Same column projection the agent repo uses, joined through the
            // project_agents membership table.
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {AGENT_COLUMNS} FROM agents a \
                     JOIN project_agents pa ON pa.agent_id = a.id \
                     WHERE pa.project_id = ?1 \
                     ORDER BY a.created_at ASC"
                ))
                .err_str()?;
            let rows: Vec<Agent> = stmt
                .query_map(params![project_id], map_agent_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn list_agents_with_meta(
        &self,
        project_id: &str,
    ) -> Result<Vec<ProjectAgentWithMeta>, String> {
        let project_id = project_id.to_string();
        self.with_conn(move |conn| {
            // Default agents float to the top so the project header row in
            // the UI is always the per-project default.
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {AGENT_COLUMNS}, pa.is_default FROM agents a \
                     JOIN project_agents pa ON pa.agent_id = a.id \
                     WHERE pa.project_id = ?1 \
                     ORDER BY pa.is_default DESC, a.created_at ASC"
                ))
                .err_str()?;
            let rows: Vec<ProjectAgentWithMeta> = stmt
                .query_map(params![project_id], |row| {
                    Ok(ProjectAgentWithMeta {
                        agent: map_agent_row(row)?,
                        // is_default is one column past the standard agent
                        // mapper's last index — keep this in sync if AGENT_COLUMNS grows.
                        is_default: row.get::<_, bool>(8)?,
                    })
                })
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn list_for_agent(&self, agent_id: &str) -> Result<Vec<Project>, String> {
        let agent_id = agent_id.to_string();
        self.with_conn(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT p.id, p.name, p.description, p.created_at, p.updated_at \
                     FROM projects p \
                     JOIN project_agents pa ON pa.project_id = p.id \
                     WHERE pa.agent_id = ?1 \
                     ORDER BY pa.added_at ASC",
                )
                .err_str()?;
            let rows: Vec<Project> = stmt
                .query_map(params![agent_id], map_project_row)
                .err_str()?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
    }

    async fn agent_in_project(&self, project_id: &str, agent_id: &str) -> Result<bool, String> {
        let project_id = project_id.to_string();
        let agent_id = agent_id.to_string();
        self.with_conn(move |conn| {
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM project_agents WHERE project_id = ?1 AND agent_id = ?2)",
                params![project_id, agent_id],
                |row| row.get::<_, bool>(0),
            )
            .err_str()
        })
        .await
    }

    async fn add_agent(
        &self,
        project_id: &str,
        agent_id: &str,
        is_default: bool,
    ) -> Result<ProjectAgent, String> {
        let project_id = project_id.to_string();
        let agent_id = agent_id.to_string();
        self.with_conn(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            // INSERT OR REPLACE so flipping the default flag on an existing
            // membership row is a single statement.
            conn.execute(
                "INSERT OR REPLACE INTO project_agents (project_id, agent_id, is_default, added_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![project_id, agent_id, is_default as i64, now],
            )
            .err_str()?;
            Ok(ProjectAgent {
                project_id,
                agent_id,
                is_default,
                added_at: now,
            })
        })
        .await
    }

    async fn remove_agent(&self, project_id: &str, agent_id: &str) -> Result<(), String> {
        let project_id = project_id.to_string();
        let agent_id = agent_id.to_string();
        self.with_conn_mut(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            let tx = conn.transaction().err_str()?;
            tx.execute(
                "DELETE FROM project_agents WHERE project_id = ?1 AND agent_id = ?2",
                params![project_id, agent_id],
            )
            .err_str()?;
            // Clear any work item assignments held by this agent in this project.
            // Cards stay in their column (no auto-move); a new claimant is
            // needed for work to continue.
            tx.execute(
                "UPDATE work_items \
                    SET assignee_agent_id = NULL, updated_at = ?1 \
                  WHERE project_id = ?2 AND assignee_agent_id = ?3",
                params![now, project_id, agent_id],
            )
            .err_str()?;
            tx.commit().err_str()?;
            Ok(())
        })
        .await
    }
}

// ── Schedules ───────────────────────────────────────────────────────────────

const SCHEDULE_COLUMNS: &str = "id, task_id, workflow_id, target_kind, kind, config, enabled, \
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
                .prepare(
                    "SELECT id, name, is_default, created_at FROM users ORDER BY created_at ASC",
                )
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

const RUN_SUMMARY_SELECT: &str = "SELECT r.id, r.task_id, t.name as task_name, r.schedule_id,
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
            let mut push_eq =
                |col: &str,
                 val: Box<dyn rusqlite::ToSql>,
                 sql: &mut String,
                 b: &mut Vec<Box<dyn rusqlite::ToSql>>| {
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

    async fn agent_conversation(&self, run_id: &str) -> Result<Option<serde_json::Value>, String> {
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

    async fn cancel(&self, run_id: &str) -> Result<(), String> {
        let run_id = run_id.to_string();
        self.with_conn(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            // Guard with state IN (...) so we don't clobber a run that
            // already finished — the cancel is a no-op in that case.
            conn.execute(
                "UPDATE runs SET state = 'cancelled', finished_at = ?1 \
                 WHERE id = ?2 AND state IN ('pending', 'queued', 'running')",
                params![now, run_id],
            )
            .err_str()?;
            Ok(())
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

    async fn update(&self, id: &str, payload: UpdateProjectBoard) -> Result<ProjectBoard, String> {
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

    async fn create_session(
        &self,
        agent_id: String,
        title: Option<String>,
        session_type: Option<String>,
        project_id: Option<String>,
    ) -> Result<ChatSession, String> {
        self.with_conn(move |conn| {
            let id = Ulid::new().to_string();
            let now = chrono::Utc::now().to_rfc3339();
            let title = title.unwrap_or_else(|| "New Chat".to_string());
            let session_type = session_type.unwrap_or_else(|| "user_chat".to_string());

            conn.execute(
                "INSERT INTO chat_sessions (
                    id, agent_id, title, archived, session_type, parent_session_id, source_bus_message_id,
                    chain_depth, execution_state, finish_summary, terminal_error, project_id, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, 0, ?4, NULL, NULL, 0, NULL, NULL, NULL, ?5, ?6, ?6)",
                params![&id, &agent_id, &title, &session_type, &project_id, &now],
            )
            .err_str()?;

            Ok(ChatSession {
                id,
                agent_id,
                title,
                archived: false,
                session_type,
                parent_session_id: None,
                source_bus_message_id: None,
                chain_depth: 0,
                execution_state: None,
                finish_summary: None,
                terminal_error: None,
                source_agent_id: None,
                source_agent_name: None,
                source_session_id: None,
                source_session_title: None,
                created_at: now.clone(),
                updated_at: now,
                project_id,
                worktree_name: None,
                worktree_branch: None,
                worktree_path: None,
            })
        })
        .await
    }

    async fn rename_session(&self, session_id: &str, title: String) -> Result<String, String> {
        let session_id = session_id.to_string();
        self.with_conn(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE chat_sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
                params![title, &now, session_id],
            )
            .err_str()?;
            Ok(now)
        })
        .await
    }

    async fn archive_session(&self, session_id: &str) -> Result<String, String> {
        let session_id = session_id.to_string();
        self.with_conn(move |conn| {
            let active_execution: Option<String> = conn
                .query_row(
                    "SELECT execution_state FROM chat_sessions WHERE id = ?1",
                    params![&session_id],
                    |row| row.get(0),
                )
                .ok();
            if matches!(
                active_execution.as_deref(),
                Some("queued") | Some("running")
            ) {
                return Err("cannot archive an active agent session".to_string());
            }

            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE chat_sessions SET archived = 1, updated_at = ?1 WHERE id = ?2",
                params![&now, &session_id],
            )
            .err_str()?;
            conn.execute(
                "UPDATE chat_sessions SET archived = 1, updated_at = ?1 WHERE parent_session_id = ?2",
                params![&now, &session_id],
            )
            .err_str()?;
            conn.execute(
                "UPDATE chat_sessions SET archived = 1, updated_at = ?1 \
                 WHERE id IN (SELECT bm.to_session_id FROM bus_messages bm WHERE bm.from_session_id = ?2 AND bm.to_session_id IS NOT NULL)",
                params![&now, &session_id],
            )
            .err_str()?;
            Ok(now)
        })
        .await
    }

    async fn unarchive_session(&self, session_id: &str) -> Result<String, String> {
        let session_id = session_id.to_string();
        self.with_conn(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE chat_sessions SET archived = 0, updated_at = ?1 WHERE id = ?2",
                params![&now, &session_id],
            )
            .err_str()?;
            conn.execute(
                "UPDATE chat_sessions SET archived = 0, updated_at = ?1 WHERE parent_session_id = ?2",
                params![&now, &session_id],
            )
            .err_str()?;
            conn.execute(
                "UPDATE chat_sessions SET archived = 0, updated_at = ?1 \
                 WHERE id IN (SELECT bm.to_session_id FROM bus_messages bm WHERE bm.from_session_id = ?2 AND bm.to_session_id IS NOT NULL)",
                params![&now, &session_id],
            )
            .err_str()?;
            Ok(now)
        })
        .await
    }

    async fn delete_session(&self, session_id: &str) -> Result<(), String> {
        let session_id = session_id.to_string();
        self.with_conn(move |conn| {
            let active_execution: Option<String> = conn
                .query_row(
                    "SELECT execution_state FROM chat_sessions WHERE id = ?1",
                    params![&session_id],
                    |row| row.get(0),
                )
                .ok();
            if matches!(
                active_execution.as_deref(),
                Some("queued") | Some("running")
            ) {
                return Err("cannot delete an active agent session".to_string());
            }
            conn.execute(
                "DELETE FROM chat_sessions WHERE id = ?1",
                params![session_id],
            )
            .err_str()?;
            Ok(())
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

    async fn session_execution(&self, session_id: &str) -> Result<SessionExecutionStatus, String> {
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

fn resolve_work_item_target_column(
    conn: &rusqlite::Connection,
    project_id: &str,
    board_id: Option<&str>,
    column_id: Option<&str>,
    status: Option<&str>,
) -> Result<ProjectBoardColumn, String> {
    resolve_board_column_sync(conn, project_id, board_id, column_id, status)
}

fn resolve_work_item_create_status(
    column: &ProjectBoardColumn,
    requested_status: Option<&str>,
) -> String {
    column
        .role
        .clone()
        .or_else(|| requested_status.map(str::to_string))
        .unwrap_or_else(|| "backlog".to_string())
}

fn resolve_work_item_move_status(column: &ProjectBoardColumn, current_status: &str) -> String {
    column
        .role
        .clone()
        .unwrap_or_else(|| current_status.to_string())
}

fn resolve_work_item_next_column(
    conn: &rusqlite::Connection,
    project_id: &str,
    board_id: Option<&str>,
    current_column_id: Option<&str>,
) -> Result<ProjectBoardColumn, String> {
    let current_column_id = current_column_id
        .ok_or_else(|| "work_item: item is not currently in a board column".to_string())?;
    let columns = list_project_board_columns_sync(conn, project_id, board_id)?;
    let current_index = columns
        .iter()
        .position(|column| column.id == current_column_id)
        .ok_or_else(|| {
            format!(
                "work_item: current board column '{}' was not found on this board",
                current_column_id
            )
        })?;
    columns
        .get(current_index + 1)
        .cloned()
        .ok_or_else(|| "work_item: item is already in the last board column".to_string())
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

    async fn lookup_project_id(&self, id: &str) -> Result<String, String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            conn.query_row(
                "SELECT project_id FROM work_items WHERE id = ?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .err_str()?
            .ok_or_else(|| format!("work item '{}' not found", id))
        })
        .await
    }

    async fn create(&self, payload: CreateWorkItem) -> Result<WorkItem, String> {
        self.with_conn(move |conn| {
            let id = Ulid::new().to_string();
            let now = chrono::Utc::now().to_rfc3339();
            let kind = payload.kind.unwrap_or_else(|| "task".to_string());
            let column = resolve_work_item_target_column(
                conn,
                &payload.project_id,
                payload.board_id.as_deref(),
                payload.column_id.as_deref(),
                payload.status.as_deref(),
            )?;
            let column_id = column.id.clone();
            let board_id = column.board_id.clone();
            let status = resolve_work_item_create_status(&column, payload.status.as_deref());
            let priority = payload.priority.unwrap_or(0);
            let position = match payload.position {
                Some(p) => p,
                None => {
                    let max: Option<f64> = conn
                        .query_row(
                            "SELECT MAX(position) FROM work_items WHERE project_id = ?1 AND column_id = ?2",
                            params![&payload.project_id, &column_id],
                            |row| row.get(0),
                        )
                        .optional()
                        .err_str()?
                        .flatten();
                    max.unwrap_or(0.0) + 1024.0
                }
            };
            let labels_json = serde_json::to_string(&payload.labels.unwrap_or_default()).err_str()?;
            let metadata_json = serde_json::to_string(
                &payload.metadata.unwrap_or_else(|| serde_json::json!({})),
            )
            .err_str()?;

            if status == "blocked" {
                return Err("work_item: cannot create a card with status='blocked' without a reason; create first then block".into());
            }

            conn.execute(
                "INSERT INTO work_items (
                    id, project_id, board_id, title, description, kind, column_id, status, priority,
                    assignee_agent_id, created_by_agent_id, parent_work_item_id, position,
                    labels, metadata, blocked_reason, started_at, completed_at, created_at, updated_at
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,NULL,NULL,NULL,?16,?16)",
                params![
                    &id,
                    &payload.project_id,
                    &board_id,
                    &payload.title,
                    &payload.description,
                    &kind,
                    &column_id,
                    &status,
                    priority,
                    &payload.assignee_agent_id,
                    &payload.created_by_agent_id,
                    &payload.parent_work_item_id,
                    position,
                    labels_json,
                    metadata_json,
                    &now,
                ],
            )
            .err_str()?;

            insert_event(
                conn,
                &id,
                Actor::System,
                event_kind::CREATED,
                serde_json::json!({
                    "title": payload.title,
                    "kind": kind,
                    "status": status,
                    "priority": priority,
                    "columnId": column_id,
                }),
            )?;

            conn.query_row(
                &format!("SELECT {WORK_ITEM_REPO_COLUMNS} FROM work_items WHERE id = ?1"),
                params![&id],
                map_work_item_repo_row,
            )
            .err_str()
        })
        .await
    }

    async fn update(&self, id: &str, payload: UpdateWorkItem) -> Result<WorkItem, String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            let before: WorkItem = conn
                .query_row(
                    &format!("SELECT {WORK_ITEM_REPO_COLUMNS} FROM work_items WHERE id = ?1"),
                    params![&id],
                    map_work_item_repo_row,
                )
                .err_str()?;
            let project_id = before.project_id.clone();

            if let Some(title) = &payload.title {
                if title.trim().is_empty() {
                    return Err("work_item: title must be non-empty".into());
                }
                if title != &before.title {
                    conn.execute(
                        "UPDATE work_items SET title = ?1, updated_at = ?2 WHERE id = ?3",
                        params![title, &now, &id],
                    )
                    .err_str()?;
                    insert_event(
                        conn,
                        &id,
                        Actor::System,
                        event_kind::TITLE_CHANGED,
                        serde_json::json!({ "from": before.title, "to": title }),
                    )?;
                }
            }
            if let Some(description) = &payload.description {
                let before_desc = before.description.clone().unwrap_or_default();
                if description != &before_desc {
                    conn.execute(
                        "UPDATE work_items SET description = ?1, updated_at = ?2 WHERE id = ?3",
                        params![description, &now, &id],
                    )
                    .err_str()?;
                    insert_event(
                        conn,
                        &id,
                        Actor::System,
                        event_kind::DESCRIPTION_CHANGED,
                        serde_json::json!({}),
                    )?;
                }
            }
            if let Some(kind) = &payload.kind {
                if kind != &before.kind {
                    conn.execute(
                        "UPDATE work_items SET kind = ?1, updated_at = ?2 WHERE id = ?3",
                        params![kind, &now, &id],
                    )
                    .err_str()?;
                    insert_event(
                        conn,
                        &id,
                        Actor::System,
                        event_kind::KIND_CHANGED,
                        serde_json::json!({ "from": before.kind, "to": kind }),
                    )?;
                }
            }
            if let Some(column_id) = payload.column_id.as_deref() {
                let resolved_column = resolve_work_item_target_column(
                    conn,
                    &project_id,
                    None,
                    Some(column_id),
                    None,
                )?;
                if Some(resolved_column.id.as_str()) != before.column_id.as_deref() {
                    conn.execute(
                        "UPDATE work_items SET column_id = ?1, updated_at = ?2 WHERE id = ?3",
                        params![&resolved_column.id, &now, &id],
                    )
                    .err_str()?;
                    insert_event(
                        conn,
                        &id,
                        Actor::System,
                        event_kind::COLUMN_CHANGED,
                        serde_json::json!({
                            "fromColumnId": before.column_id,
                            "toColumnId": resolved_column.id,
                            "toColumnName": resolved_column.name,
                        }),
                    )?;
                }
            }
            if let Some(priority) = payload.priority {
                if priority != before.priority {
                    conn.execute(
                        "UPDATE work_items SET priority = ?1, updated_at = ?2 WHERE id = ?3",
                        params![priority, &now, &id],
                    )
                    .err_str()?;
                    insert_event(
                        conn,
                        &id,
                        Actor::System,
                        event_kind::PRIORITY_CHANGED,
                        serde_json::json!({ "from": before.priority, "to": priority }),
                    )?;
                }
            }
            if let Some(labels) = &payload.labels {
                if labels != &before.labels {
                    let labels_json = serde_json::to_string(labels).err_str()?;
                    conn.execute(
                        "UPDATE work_items SET labels = ?1, updated_at = ?2 WHERE id = ?3",
                        params![labels_json, &now, &id],
                    )
                    .err_str()?;
                    insert_event(
                        conn,
                        &id,
                        Actor::System,
                        event_kind::LABELS_CHANGED,
                        serde_json::json!({ "from": before.labels, "to": labels }),
                    )?;
                }
            }
            if let Some(metadata) = &payload.metadata {
                let metadata_json = serde_json::to_string(metadata).err_str()?;
                conn.execute(
                    "UPDATE work_items SET metadata = ?1, updated_at = ?2 WHERE id = ?3",
                    params![metadata_json, &now, &id],
                )
                .err_str()?;
            }

            conn.query_row(
                &format!("SELECT {WORK_ITEM_REPO_COLUMNS} FROM work_items WHERE id = ?1"),
                params![&id],
                map_work_item_repo_row,
            )
            .err_str()
        })
        .await
    }

    async fn delete(&self, id: &str) -> Result<(), String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            conn.execute("DELETE FROM work_items WHERE id = ?1", params![id])
                .err_str()?;
            Ok(())
        })
        .await
    }

    async fn claim(&self, id: &str, agent_id: &str) -> Result<WorkItem, String> {
        let id = id.to_string();
        let agent_id = agent_id.to_string();
        self.with_conn(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            let before: WorkItem = conn
                .query_row(
                    &format!("SELECT {WORK_ITEM_REPO_COLUMNS} FROM work_items WHERE id = ?1"),
                    params![&id],
                    map_work_item_repo_row,
                )
                .err_str()?;
            let column = resolve_work_item_target_column(
                conn,
                &before.project_id,
                before.board_id.as_deref(),
                None,
                Some("in_progress"),
            )?;
            let column_id = column.id.clone();
            let status = column
                .role
                .clone()
                .unwrap_or_else(|| "in_progress".to_string());
            conn.execute(
                "UPDATE work_items
                    SET assignee_agent_id = ?1,
                        column_id = ?2,
                        status = ?3,
                        blocked_reason = NULL,
                        started_at = COALESCE(started_at, ?4),
                        updated_at = ?4
                  WHERE id = ?5",
                params![&agent_id, &column_id, &status, &now, &id],
            )
            .err_str()?;

            if before.assignee_agent_id.as_deref() != Some(agent_id.as_str()) {
                insert_event(
                    conn,
                    &id,
                    Actor::System,
                    event_kind::ASSIGNEE_CHANGED,
                    serde_json::json!({
                        "fromAgentId": before.assignee_agent_id,
                        "toAgentId": agent_id,
                    }),
                )?;
            }
            if before.column_id.as_deref() != Some(column_id.as_str()) {
                insert_event(
                    conn,
                    &id,
                    Actor::System,
                    event_kind::COLUMN_CHANGED,
                    serde_json::json!({
                        "fromColumnId": before.column_id,
                        "toColumnId": column_id,
                        "toColumnName": column.name,
                        "reason": "claim",
                    }),
                )?;
            }

            conn.query_row(
                &format!("SELECT {WORK_ITEM_REPO_COLUMNS} FROM work_items WHERE id = ?1"),
                params![&id],
                map_work_item_repo_row,
            )
            .err_str()
        })
        .await
    }

    async fn move_item(
        &self,
        id: &str,
        column_id: Option<String>,
        position: Option<f64>,
    ) -> Result<WorkItem, String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            let before: WorkItem = conn
                .query_row(
                    &format!("SELECT {WORK_ITEM_REPO_COLUMNS} FROM work_items WHERE id = ?1"),
                    params![&id],
                    map_work_item_repo_row,
                )
                .err_str()?;
            let column = match column_id.as_deref() {
                Some(column_id) => resolve_work_item_target_column(
                    conn,
                    &before.project_id,
                    before.board_id.as_deref(),
                    Some(column_id),
                    None,
                )?,
                None => resolve_work_item_next_column(
                    conn,
                    &before.project_id,
                    before.board_id.as_deref(),
                    before.column_id.as_deref(),
                )?,
            };
            let column_id = column.id.clone();
            let status = resolve_work_item_move_status(&column, &before.status);

            if status == "blocked" {
                let reason_ok: bool = conn
                    .query_row(
                        "SELECT blocked_reason IS NOT NULL AND length(blocked_reason) > 0
                           FROM work_items WHERE id = ?1",
                        params![&id],
                        |row| row.get(0),
                    )
                    .err_str()?;
                if !reason_ok {
                    return Err(
                        "work_item: moving to 'blocked' requires a non-empty blocked_reason; use block() first"
                            .into(),
                    );
                }
            }

            let position = match position {
                Some(p) => p,
                None => {
                    if before.status == status
                        && before.column_id.as_deref() == Some(column_id.as_str())
                    {
                        before.position
                    } else {
                        let max: Option<f64> = conn
                            .query_row(
                                "SELECT MAX(position) FROM work_items WHERE project_id = ?1 AND column_id = ?2",
                                params![&before.project_id, &column_id],
                                |row| row.get(0),
                            )
                            .optional()
                            .err_str()?
                            .flatten();
                        max.unwrap_or(0.0) + 1024.0
                    }
                }
            };

            let started_at_expr = if before.status != "in_progress" && status == "in_progress" {
                "COALESCE(started_at, ?4)"
            } else {
                "started_at"
            };
            let completed_at_expr = if status == "done" || status == "cancelled" {
                "?4"
            } else {
                "completed_at"
            };
            let blocked_reason_expr = if before.status == "blocked" && status != "blocked" {
                "NULL"
            } else {
                "blocked_reason"
            };

            let sql = format!(
                "UPDATE work_items
                    SET column_id = ?1,
                        status = ?2,
                        position = ?3,
                        started_at = {},
                        completed_at = {},
                        blocked_reason = {},
                        updated_at = ?5
                  WHERE id = ?4",
                started_at_expr, completed_at_expr, blocked_reason_expr
            );
            conn.execute(&sql, params![&column_id, &status, position, &id, &now])
                .err_str()?;

            if before.column_id.as_deref() != Some(column_id.as_str()) {
                insert_event(
                    conn,
                    &id,
                    Actor::System,
                    event_kind::COLUMN_CHANGED,
                    serde_json::json!({
                        "fromColumnId": before.column_id,
                        "fromStatus": before.status,
                        "toColumnId": column_id,
                        "toColumnName": column.name,
                        "toStatus": status,
                    }),
                )?;
            }
            if status == "done" && before.status != "done" {
                insert_event(
                    conn,
                    &id,
                    Actor::System,
                    event_kind::COMPLETED,
                    serde_json::json!({ "via": "move" }),
                )?;
            }
            if before.status == "blocked" && status != "blocked" {
                insert_event(
                    conn,
                    &id,
                    Actor::System,
                    event_kind::UNBLOCKED,
                    serde_json::json!({ "via": "move" }),
                )?;
            }

            conn.query_row(
                &format!("SELECT {WORK_ITEM_REPO_COLUMNS} FROM work_items WHERE id = ?1"),
                params![&id],
                map_work_item_repo_row,
            )
            .err_str()
        })
        .await
    }

    async fn reorder(
        &self,
        project_id: &str,
        board_id: Option<String>,
        status: Option<String>,
        column_id: Option<String>,
        ordered_ids: Vec<String>,
    ) -> Result<(), String> {
        let project_id = project_id.to_string();
        self.with_conn_mut(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            let resolved_column_id = resolve_work_item_target_column(
                conn,
                &project_id,
                board_id.as_deref(),
                column_id.as_deref(),
                status.as_deref(),
            )?
            .id;
            let tx = conn.transaction().err_str()?;
            for (idx, item_id) in ordered_ids.iter().enumerate() {
                let pos = ((idx + 1) as f64) * 1024.0;
                tx.execute(
                    "UPDATE work_items
                        SET position = ?1, updated_at = ?2
                      WHERE id = ?3 AND project_id = ?4 AND column_id = ?5",
                    params![pos, &now, item_id, &project_id, &resolved_column_id],
                )
                .err_str()?;
            }
            tx.commit().err_str()?;
            Ok(())
        })
        .await
    }

    async fn block(&self, id: &str, reason: String) -> Result<WorkItem, String> {
        if reason.trim().is_empty() {
            return Err("work_item: blocked_reason must be non-empty".into());
        }
        let id = id.to_string();
        self.with_conn(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            let (project_id, board_id): (String, Option<String>) = conn
                .query_row(
                    "SELECT project_id, board_id FROM work_items WHERE id = ?1",
                    params![&id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .err_str()?;
            let column = resolve_work_item_target_column(
                conn,
                &project_id,
                board_id.as_deref(),
                None,
                Some("blocked"),
            )?;
            let column_id = column.id;
            let status = column.role.unwrap_or_else(|| "blocked".to_string());
            conn.execute(
                "UPDATE work_items
                    SET column_id = ?1, status = ?2, blocked_reason = ?3, updated_at = ?4
                  WHERE id = ?5",
                params![&column_id, &status, &reason, &now, &id],
            )
            .err_str()?;
            insert_event(
                conn,
                &id,
                Actor::System,
                event_kind::BLOCKED,
                serde_json::json!({ "reason": reason }),
            )?;
            conn.query_row(
                &format!("SELECT {WORK_ITEM_REPO_COLUMNS} FROM work_items WHERE id = ?1"),
                params![&id],
                map_work_item_repo_row,
            )
            .err_str()
        })
        .await
    }

    async fn unblock(&self, id: &str, status: String) -> Result<WorkItem, String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            let (project_id, board_id): (String, Option<String>) = conn
                .query_row(
                    "SELECT project_id, board_id FROM work_items WHERE id = ?1",
                    params![&id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .err_str()?;
            let column = resolve_work_item_target_column(
                conn,
                &project_id,
                board_id.as_deref(),
                None,
                Some(&status),
            )?;
            let column_id = column.id;
            let resolved_status = column.role.unwrap_or(status);
            conn.execute(
                "UPDATE work_items
                    SET column_id = ?1,
                        status = ?2,
                        blocked_reason = NULL,
                        updated_at = ?3
                  WHERE id = ?4",
                params![&column_id, &resolved_status, &now, &id],
            )
            .err_str()?;
            insert_event(
                conn,
                &id,
                Actor::System,
                event_kind::UNBLOCKED,
                serde_json::json!({ "toStatus": resolved_status }),
            )?;
            conn.query_row(
                &format!("SELECT {WORK_ITEM_REPO_COLUMNS} FROM work_items WHERE id = ?1"),
                params![&id],
                map_work_item_repo_row,
            )
            .err_str()
        })
        .await
    }

    async fn complete(&self, id: &str) -> Result<WorkItem, String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            let (project_id, board_id): (String, Option<String>) = conn
                .query_row(
                    "SELECT project_id, board_id FROM work_items WHERE id = ?1",
                    params![&id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .err_str()?;
            let column = resolve_work_item_target_column(
                conn,
                &project_id,
                board_id.as_deref(),
                None,
                Some("done"),
            )?;
            let column_id = column.id;
            let status = column.role.unwrap_or_else(|| "done".to_string());
            conn.execute(
                "UPDATE work_items
                    SET column_id = ?1,
                        status = ?2,
                        completed_at = ?3,
                        blocked_reason = NULL,
                        updated_at = ?3
                  WHERE id = ?4",
                params![&column_id, &status, &now, &id],
            )
            .err_str()?;
            insert_event(
                conn,
                &id,
                Actor::System,
                event_kind::COMPLETED,
                serde_json::json!({ "via": "complete" }),
            )?;
            conn.query_row(
                &format!("SELECT {WORK_ITEM_REPO_COLUMNS} FROM work_items WHERE id = ?1"),
                params![&id],
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

    async fn create_comment(
        &self,
        work_item_id: &str,
        body: String,
        author: CommentAuthor,
    ) -> Result<WorkItemComment, String> {
        if body.trim().is_empty() {
            return Err("work_item_comment: body must be non-empty".into());
        }
        let work_item_id = work_item_id.to_string();
        self.with_conn(move |conn| {
            let id = Ulid::new().to_string();
            let now = chrono::Utc::now().to_rfc3339();
            let (author_kind, author_agent_id) = match author {
                CommentAuthor::User => ("user", None),
                CommentAuthor::Agent { agent_id } => ("agent", Some(agent_id)),
            };
            conn.execute(
                "INSERT INTO work_item_comments (
                    id, work_item_id, author_kind, author_agent_id, body, created_at, updated_at
                 ) VALUES (?1,?2,?3,?4,?5,?6,?6)",
                params![
                    &id,
                    &work_item_id,
                    author_kind,
                    &author_agent_id,
                    &body,
                    &now
                ],
            )
            .err_str()?;
            let actor = match author_kind {
                "agent" => match author_agent_id.as_deref() {
                    Some(aid) => Actor::Agent { agent_id: aid },
                    None => Actor::System,
                },
                "user" => Actor::User,
                _ => Actor::System,
            };
            insert_event(
                conn,
                &work_item_id,
                actor,
                event_kind::COMMENT_ADDED,
                serde_json::json!({ "commentId": id }),
            )?;
            conn.query_row(
                &format!(
                    "SELECT {WORK_ITEM_COMMENT_REPO_COLUMNS} FROM work_item_comments WHERE id = ?1"
                ),
                params![&id],
                map_work_item_comment_row,
            )
            .err_str()
        })
        .await
    }

    async fn update_comment(&self, id: &str, body: String) -> Result<WorkItemComment, String> {
        if body.trim().is_empty() {
            return Err("work_item_comment: body must be non-empty".into());
        }
        let id = id.to_string();
        self.with_conn(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE work_item_comments SET body = ?1, updated_at = ?2 WHERE id = ?3",
                params![&body, &now, &id],
            )
            .err_str()?;
            let comment: WorkItemComment = conn
                .query_row(
                    &format!(
                        "SELECT {WORK_ITEM_COMMENT_REPO_COLUMNS} FROM work_item_comments WHERE id = ?1"
                    ),
                    params![&id],
                    map_work_item_comment_row,
                )
                .err_str()?;
            insert_event(
                conn,
                &comment.work_item_id,
                Actor::System,
                event_kind::COMMENT_EDITED,
                serde_json::json!({ "commentId": comment.id }),
            )?;
            Ok(comment)
        })
        .await
    }

    async fn delete_comment(&self, id: &str) -> Result<(), String> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            let work_item_id: Option<String> = conn
                .query_row(
                    "SELECT work_item_id FROM work_item_comments WHERE id = ?1",
                    params![&id],
                    |row| row.get(0),
                )
                .optional()
                .err_str()?;
            conn.execute("DELETE FROM work_item_comments WHERE id = ?1", params![&id])
                .err_str()?;
            if let Some(wid) = work_item_id {
                insert_event(
                    conn,
                    &wid,
                    Actor::System,
                    event_kind::COMMENT_DELETED,
                    serde_json::json!({ "commentId": id }),
                )?;
            }
            Ok(())
        })
        .await
    }
}

// ── Project workflows ───────────────────────────────────────────────────────
//
// The write surface owns graph normalization, trigger reconciliation, and the
// transactional graph swap so command/tool call sites do not need `DbPool`.

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

fn validate_project_workflow_graph(graph: &WorkflowGraph) -> Result<(), String> {
    let node_ids: HashSet<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    if node_ids.len() != graph.nodes.len() {
        return Err("workflow: duplicate node ids".into());
    }

    let mut reference_keys = HashSet::new();
    for node in &graph.nodes {
        if !KNOWN_NODE_TYPES.contains(&node.node_type.as_str()) {
            return Err(format!(
                "workflow: unknown node type '{}' on node '{}'",
                node.node_type, node.id
            ));
        }
        if let Some(reference_key) = node
            .data
            .get("referenceKey")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if !is_valid_workflow_reference_key(reference_key) {
                return Err(format!(
                    "workflow: node '{}' has invalid referenceKey '{}'; use kebab-case letters, numbers, and hyphens",
                    node.id, reference_key
                ));
            }
            if !reference_keys.insert(reference_key.to_string()) {
                return Err(format!(
                    "workflow: duplicate referenceKey '{}' found; each node reference name must be unique",
                    reference_key
                ));
            }
        }
        if node.node_type == "logic.if" {
            if let Some(rule_value) = node.data.get("rule") {
                let parsed: Result<RuleNode, _> = serde_json::from_value(rule_value.clone());
                match parsed {
                    Ok(rule) => validate_project_workflow_rule(&rule)?,
                    Err(e) => {
                        return Err(format!(
                            "workflow: logic.if node '{}' has malformed rule: {}",
                            node.id, e
                        ))
                    }
                }
            }
        }
    }

    let mut incoming: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut logic_if_handles: std::collections::HashMap<&str, HashSet<String>> =
        std::collections::HashMap::new();
    let logic_if_ids: HashSet<&str> = graph
        .nodes
        .iter()
        .filter(|n| n.node_type == "logic.if")
        .map(|n| n.id.as_str())
        .collect();

    for edge in &graph.edges {
        if !node_ids.contains(edge.source.as_str()) {
            return Err(format!(
                "workflow: edge '{}' references unknown source node '{}'",
                edge.id, edge.source
            ));
        }
        if !node_ids.contains(edge.target.as_str()) {
            return Err(format!(
                "workflow: edge '{}' references unknown target node '{}'",
                edge.id, edge.target
            ));
        }
        *incoming.entry(edge.target.as_str()).or_insert(0) += 1;
        if logic_if_ids.contains(edge.source.as_str()) {
            let handle = edge.source_handle.clone().unwrap_or_default();
            if handle != "true" && handle != "false" {
                return Err(format!(
                    "workflow: logic.if node '{}' has outgoing edge '{}' with invalid handle '{}', expected 'true' or 'false'",
                    edge.source, edge.id, handle
                ));
            }
            let inserted = logic_if_handles
                .entry(edge.source.as_str())
                .or_default()
                .insert(handle.clone());
            if !inserted {
                return Err(format!(
                    "workflow: logic.if node '{}' has multiple outgoing '{}' edges; each branch may only connect once",
                    edge.source, handle
                ));
            }
        }
    }

    for (target, count) in &incoming {
        if *count > 1 {
            return Err(format!(
                "workflow: node '{}' has {} incoming edges; fan-in / join nodes are not supported",
                target, count
            ));
        }
    }

    Ok(())
}

fn is_valid_workflow_reference_key(value: &str) -> bool {
    if value == "trigger" || value == "__aliases" || value.starts_with('-') || value.ends_with('-')
    {
        return false;
    }
    let mut saw_alnum = false;
    for ch in value.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            saw_alnum = true;
            continue;
        }
        if ch != '-' {
            return false;
        }
    }
    saw_alnum
}

fn validate_project_workflow_rule(rule: &RuleNode) -> Result<(), String> {
    match rule {
        RuleNode::Group(group) => {
            if group.combinator != "and" && group.combinator != "or" {
                return Err(format!(
                    "workflow: unknown rule combinator '{}'",
                    group.combinator
                ));
            }
            for child in &group.rules {
                validate_project_workflow_rule(child)?;
            }
            Ok(())
        }
        RuleNode::Leaf(leaf) => {
            if !RULE_OPERATORS.contains(&leaf.operator.as_str()) {
                return Err(format!(
                    "workflow: unknown rule operator '{}'",
                    leaf.operator
                ));
            }
            Ok(())
        }
    }
}

fn derive_project_workflow_trigger(
    graph: &WorkflowGraph,
    fallback_kind: Option<&str>,
    fallback_config: Option<&serde_json::Value>,
) -> (String, serde_json::Value) {
    if let Some(node) = graph
        .nodes
        .iter()
        .find(|node| node.node_type == "trigger.schedule")
    {
        return ("schedule".to_string(), node.data.clone());
    }
    if graph
        .nodes
        .iter()
        .any(|node| node.node_type == "trigger.manual")
    {
        return ("manual".to_string(), serde_json::json!({}));
    }
    (
        fallback_kind.unwrap_or("manual").to_string(),
        fallback_config
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    )
}

fn normalize_project_workflow_data_object(
    value: &serde_json::Value,
) -> serde_json::Map<String, serde_json::Value> {
    match value {
        serde_json::Value::Object(map) => map.clone(),
        _ => serde_json::Map::new(),
    }
}

fn slugify_project_workflow_reference_key(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.trim().to_lowercase().chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn normalize_project_workflow_reference_key(value: Option<&str>) -> Option<String> {
    let normalized = slugify_project_workflow_reference_key(value.unwrap_or_default());
    if normalized.is_empty() || normalized == "trigger" || normalized == "__aliases" {
        return None;
    }
    Some(normalized)
}

fn project_workflow_node_label(node_type: &str) -> String {
    match node_type {
        "trigger.manual" => "Run now".to_string(),
        "trigger.schedule" => "Schedule".to_string(),
        "agent.run" => "Run agent".to_string(),
        "logic.if" => "If / branch".to_string(),
        "code.bash.run" => "Code Bash".to_string(),
        "code.script.run" => "Code JS/TS".to_string(),
        "board.work_item.create" => "Board Work item".to_string(),
        "board.proposal.enqueue" => "Board Proposal queue".to_string(),
        "integration.feed.fetch" => "Feed fetch".to_string(),
        "integration.com_orbit_discord.send_message" => "Discord Send message".to_string(),
        "integration.gmail.read" => "Gmail Read".to_string(),
        "integration.gmail.send" => "Gmail Send".to_string(),
        "integration.slack.send" => "Slack Send".to_string(),
        "integration.http.request" => "HTTP request".to_string(),
        other => other.replace('.', " "),
    }
}

fn project_workflow_reference_base(node_type: &str, preferred: Option<&str>) -> String {
    normalize_project_workflow_reference_key(preferred).unwrap_or_else(|| {
        slugify_project_workflow_reference_key(&project_workflow_node_label(node_type))
    })
}

fn is_generated_project_workflow_reference_key(node_type: &str, value: &str) -> bool {
    let base = project_workflow_reference_base(node_type, None);
    value == base
        || value
            .strip_prefix(&(base + "-"))
            .map(|suffix| suffix.chars().all(|ch| ch.is_ascii_digit()))
            .unwrap_or(false)
}

fn project_workflow_node_has_linked_outputs(
    node_id: &str,
    node_type: &str,
    edges: &[WorkflowEdge],
) -> bool {
    node_type.starts_with("trigger.") || edges.iter().any(|edge| edge.source == node_id)
}

fn normalize_project_workflow_graph_for_storage(graph: &WorkflowGraph) -> WorkflowGraph {
    let mut normalized = graph.clone();
    let mut used = HashSet::from(["trigger".to_string(), "__aliases".to_string()]);

    for node in &normalized.nodes {
        if !project_workflow_node_has_linked_outputs(&node.id, &node.node_type, &normalized.edges) {
            continue;
        }
        let data = normalize_project_workflow_data_object(&node.data);
        let Some(existing) = data.get("referenceKey").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(existing) = normalize_project_workflow_reference_key(Some(existing)) else {
            continue;
        };
        if !is_generated_project_workflow_reference_key(&node.node_type, &existing) {
            used.insert(existing);
        }
    }

    for node in &mut normalized.nodes {
        let mut data = normalize_project_workflow_data_object(&node.data);
        let existing = data
            .get("referenceKey")
            .and_then(serde_json::Value::as_str)
            .and_then(|value| normalize_project_workflow_reference_key(Some(value)));

        if !project_workflow_node_has_linked_outputs(&node.id, &node.node_type, &normalized.edges) {
            node.data = serde_json::Value::Object(data);
            continue;
        }

        if let Some(existing) = existing {
            if !is_generated_project_workflow_reference_key(&node.node_type, &existing) {
                data.insert(
                    "referenceKey".to_string(),
                    serde_json::Value::String(existing),
                );
                node.data = serde_json::Value::Object(data);
                continue;
            }
        }

        let base = project_workflow_reference_base(&node.node_type, None);
        let mut suffix = 1usize;
        let mut candidate = format!("{}-{}", base, suffix);
        while used.contains(&candidate) {
            suffix += 1;
            candidate = format!("{}-{}", base, suffix);
        }
        used.insert(candidate.clone());
        data.insert(
            "referenceKey".to_string(),
            serde_json::Value::String(candidate),
        );
        node.data = serde_json::Value::Object(data);
    }

    normalized
}

fn prepare_project_workflow_for_write(
    graph: &WorkflowGraph,
    fallback_kind: Option<&str>,
    fallback_config: Option<&serde_json::Value>,
) -> Result<(WorkflowGraph, String, serde_json::Value), String> {
    let graph = normalize_project_workflow_graph_for_storage(graph);
    validate_project_workflow_graph(&graph)?;
    let (trigger_kind, trigger_config) =
        derive_project_workflow_trigger(&graph, fallback_kind, fallback_config);
    Ok((graph, trigger_kind, trigger_config))
}

fn structured_project_workflow_error(code: &str, message: String) -> String {
    serde_json::json!({
        "code": code,
        "message": message,
    })
    .to_string()
}

fn ensure_project_workflow_can_enable_or_run(
    graph: &WorkflowGraph,
    action: &str,
) -> Result<(), String> {
    if graph
        .nodes
        .iter()
        .any(|node| node.node_type.starts_with("trigger."))
    {
        return Ok(());
    }
    Err(structured_project_workflow_error(
        "workflow_missing_trigger",
        format!("workflow cannot {} without a trigger node", action),
    ))
}

fn sync_project_workflow_schedule(
    conn: &rusqlite::Connection,
    workflow_id: &str,
    enabled: bool,
    trigger_kind: &str,
    trigger_config: &serde_json::Value,
    now: &str,
) -> Result<(), String> {
    let schedule_id = format!("workflow-schedule-{}", workflow_id);
    if !enabled || trigger_kind != "schedule" {
        conn.execute(
            "DELETE FROM schedules WHERE workflow_id = ?1",
            params![workflow_id],
        )
        .err_str()?;
        return Ok(());
    }

    let config: RecurringConfig = serde_json::from_value(trigger_config.clone())
        .map_err(|e| format!("workflow schedule config is invalid: {}", e))?;
    let next_run_at = next_n_runs(&config, 1).into_iter().next();
    let config_json = serde_json::to_string(trigger_config).err_str()?;

    conn.execute(
        "INSERT OR REPLACE INTO schedules (
            id, task_id, workflow_id, target_kind, kind, config, enabled,
            next_run_at, last_run_at, created_at, updated_at
         ) VALUES (?1, NULL, ?2, 'workflow', 'recurring', ?3, 1, ?4, NULL,
                   COALESCE((SELECT created_at FROM schedules WHERE id = ?1), ?5), ?5)",
        params![schedule_id, workflow_id, config_json, next_run_at, now],
    )
    .err_str()?;

    Ok(())
}

#[async_trait]
impl ProjectWorkflowRepo for SqliteRepos {
    async fn list(&self, project_id: &str, limit: i64) -> Result<Vec<ProjectWorkflow>, String> {
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
                &format!("SELECT {PROJECT_WORKFLOW_COLUMNS} FROM project_workflows WHERE id = ?1"),
                params![id],
                map_project_workflow_row,
            )
            .err_str()
        })
        .await
    }

    async fn create(&self, payload: CreateProjectWorkflow) -> Result<ProjectWorkflow, String> {
        self.with_conn_mut(move |conn| {
            let graph = payload.graph.unwrap_or_default();
            let (graph, trigger_kind, trigger_config) = prepare_project_workflow_for_write(
                &graph,
                payload.trigger_kind.as_deref(),
                payload.trigger_config.as_ref(),
            )?;

            let tx = conn.transaction().err_str()?;
            let id = Ulid::new().to_string();
            let now = chrono::Utc::now().to_rfc3339();
            let graph_json = serde_json::to_string(&graph).err_str()?;
            let trigger_config_json = serde_json::to_string(&trigger_config).err_str()?;

            tx.execute(
                "INSERT INTO project_workflows (
                    id, project_id, name, description, enabled, graph,
                    trigger_kind, trigger_config, version, created_at, updated_at
                 ) VALUES (?1,?2,?3,?4,0,?5,?6,?7,1,?8,?8)",
                params![
                    &id,
                    &payload.project_id,
                    &payload.name,
                    &payload.description,
                    graph_json,
                    &trigger_kind,
                    trigger_config_json,
                    &now,
                ],
            )
            .err_str()?;

            sync_project_workflow_schedule(&tx, &id, false, &trigger_kind, &trigger_config, &now)?;

            let item = tx
                .query_row(
                    &format!(
                        "SELECT {PROJECT_WORKFLOW_COLUMNS} FROM project_workflows WHERE id = ?1"
                    ),
                    params![&id],
                    map_project_workflow_row,
                )
                .err_str()?;
            tx.commit().err_str()?;
            Ok(item)
        })
        .await
    }

    async fn update(
        &self,
        id: &str,
        payload: UpdateProjectWorkflow,
    ) -> Result<ProjectWorkflow, String> {
        let id = id.to_string();
        self.with_conn_mut(move |conn| {
            let tx = conn.transaction().err_str()?;
            let now = chrono::Utc::now().to_rfc3339();

            let current = tx
                .query_row(
                    &format!(
                        "SELECT {PROJECT_WORKFLOW_COLUMNS} FROM project_workflows WHERE id = ?1"
                    ),
                    params![&id],
                    map_project_workflow_row,
                )
                .err_str()?;

            if let Some(name) = &payload.name {
                if name.trim().is_empty() {
                    return Err("workflow: name must be non-empty".into());
                }
                tx.execute(
                    "UPDATE project_workflows SET name = ?1, updated_at = ?2 WHERE id = ?3",
                    params![name, &now, &id],
                )
                .err_str()?;
            }
            if let Some(description) = &payload.description {
                tx.execute(
                    "UPDATE project_workflows SET description = ?1, updated_at = ?2 WHERE id = ?3",
                    params![description, &now, &id],
                )
                .err_str()?;
            }

            let normalized_graph = if let Some(graph) = &payload.graph {
                let (graph, _, _) = prepare_project_workflow_for_write(
                    graph,
                    payload
                        .trigger_kind
                        .as_deref()
                        .or(Some(current.trigger_kind.as_str())),
                    payload.trigger_config.as_ref().or(Some(&current.trigger_config)),
                )?;
                let json = serde_json::to_string(&graph).err_str()?;
                tx.execute(
                    "UPDATE project_workflows
                        SET graph = ?1, version = version + 1, updated_at = ?2
                      WHERE id = ?3",
                    params![json, &now, &id],
                )
                .err_str()?;
                Some(graph)
            } else {
                None
            };

            let graph_for_trigger = normalized_graph.as_ref().unwrap_or(&current.graph);
            let (trigger_kind, trigger_config) = derive_project_workflow_trigger(
                graph_for_trigger,
                payload
                    .trigger_kind
                    .as_deref()
                    .or(Some(current.trigger_kind.as_str())),
                payload.trigger_config.as_ref().or(Some(&current.trigger_config)),
            );
            let trigger_config_json = serde_json::to_string(&trigger_config).err_str()?;
            tx.execute(
                "UPDATE project_workflows SET trigger_kind = ?1, trigger_config = ?2, updated_at = ?3 WHERE id = ?4",
                params![&trigger_kind, trigger_config_json, &now, &id],
            )
            .err_str()?;
            sync_project_workflow_schedule(
                &tx,
                &current.id,
                current.enabled,
                &trigger_kind,
                &trigger_config,
                &now,
            )?;

            let item = tx
                .query_row(
                    &format!(
                        "SELECT {PROJECT_WORKFLOW_COLUMNS} FROM project_workflows WHERE id = ?1"
                    ),
                    params![&id],
                    map_project_workflow_row,
                )
                .err_str()?;
            tx.commit().err_str()?;
            Ok(item)
        })
        .await
    }

    async fn delete(&self, id: &str) -> Result<(), String> {
        let workflow_id = id.to_string();
        self.with_conn_mut(move |conn| {
            let tx = conn.transaction().err_str()?;
            tx.execute(
                "DELETE FROM schedules WHERE workflow_id = ?1",
                params![&workflow_id],
            )
            .err_str()?;
            tx.execute(
                "DELETE FROM project_workflows WHERE id = ?1",
                params![&workflow_id],
            )
            .err_str()?;
            tx.commit().err_str()?;
            Ok(())
        })
        .await
    }

    async fn set_enabled(&self, id: &str, enabled: bool) -> Result<ProjectWorkflow, String> {
        let id = id.to_string();
        self.with_conn_mut(move |conn| {
            let tx = conn.transaction().err_str()?;
            let current = tx
                .query_row(
                    &format!(
                        "SELECT {PROJECT_WORKFLOW_COLUMNS} FROM project_workflows WHERE id = ?1"
                    ),
                    params![&id],
                    map_project_workflow_row,
                )
                .err_str()?;
            if enabled {
                ensure_project_workflow_can_enable_or_run(&current.graph, "enable")?;
            }

            let now = chrono::Utc::now().to_rfc3339();
            let flag: i64 = if enabled { 1 } else { 0 };
            tx.execute(
                "UPDATE project_workflows SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
                params![flag, &now, &id],
            )
            .err_str()?;
            sync_project_workflow_schedule(
                &tx,
                &current.id,
                enabled,
                &current.trigger_kind,
                &current.trigger_config,
                &now,
            )?;
            let item = tx
                .query_row(
                    &format!(
                        "SELECT {PROJECT_WORKFLOW_COLUMNS} FROM project_workflows WHERE id = ?1"
                    ),
                    params![&id],
                    map_project_workflow_row,
                )
                .err_str()?;
            tx.commit().err_str()?;
            Ok(item)
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

fn map_project_board_column_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectBoardColumn> {
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
