//! Per-aggregate repository traits.
//!
//! Every database-touching command will eventually call these traits instead
//! of holding a `DbPool` directly. Two backends:
//!
//! * `sqlite::SqliteRepos` (this crate) — wraps the existing rusqlite/r2d2
//!   pool. Used by the desktop app, the standalone server in single-tenant
//!   mode, and per-tenant Fly Machines on the paid SaaS tier.
//! * `postgres::PgRepos` (added in Phase C) — wraps a sqlx `PgPool`. Used by
//!   the shared multi-tenant runtime tier (free SaaS), with row-level
//!   security scoped on every connection by tenant_id.
//!
//! Aggregate-by-aggregate migration: the trait surface here grows as
//! `commands/{tasks,agents,runs,…}` are switched over from direct `DbPool`
//! access. Once nothing references `DbPool` directly, rusqlite/r2d2 are
//! removed and `SqliteRepos` is rewritten on top of `sqlx::SqlitePool`.

pub mod sqlite;

use async_trait::async_trait;

use crate::models::agent::{Agent, CreateAgent, UpdateAgent};
use crate::models::project::{CreateProject, Project, ProjectSummary, UpdateProject};
use crate::models::schedule::{CreateSchedule, Schedule};
use crate::models::task::{CreateTask, Task, UpdateTask};
use crate::models::user::User;

/// Top-level repository facade. The concrete impl picks the backend.
pub trait Repos: Send + Sync {
    fn agents(&self) -> &dyn AgentRepo;
    fn projects(&self) -> &dyn ProjectRepo;
    fn schedules(&self) -> &dyn ScheduleRepo;
    fn tasks(&self) -> &dyn TaskRepo;
    fn users(&self) -> &dyn UserRepo;
}

#[async_trait]
pub trait AgentRepo: Send + Sync {
    async fn list(&self) -> Result<Vec<Agent>, String>;
    async fn get(&self, id: &str) -> Result<Option<Agent>, String>;
    /// Inserts a new agent. Returns the new ID alongside the inserted row;
    /// callers run filesystem-side workspace setup, then `set_model_config`.
    async fn create_basic(&self, payload: CreateAgent) -> Result<Agent, String>;
    async fn set_model_config(&self, id: &str, model_config_json: &str) -> Result<(), String>;
    /// Patches the editable scalar fields. Slug renames + cross-table
    /// reference updates remain on the legacy `DbPool` path until an
    /// explicit `rename_with_references` repo method exists.
    async fn update_basic(&self, id: &str, payload: UpdateAgent) -> Result<Agent, String>;
    async fn delete(&self, id: &str) -> Result<(), String>;
    async fn next_available_id(
        &self,
        name: &str,
        current_id: Option<&str>,
    ) -> Result<String, String>;
}

#[async_trait]
pub trait ProjectRepo: Send + Sync {
    async fn list(&self) -> Result<Vec<ProjectSummary>, String>;
    async fn get(&self, id: &str) -> Result<Option<Project>, String>;
    /// Inserts the projects row only. Callers run board-column scaffolding
    /// and workspace init.
    async fn create_basic(&self, payload: CreateProject) -> Result<Project, String>;
    async fn update(&self, id: &str, payload: UpdateProject) -> Result<Project, String>;
    async fn delete(&self, id: &str) -> Result<(), String>;
}

#[async_trait]
pub trait ScheduleRepo: Send + Sync {
    async fn list(&self) -> Result<Vec<Schedule>, String>;
    async fn list_for_task(&self, task_id: &str) -> Result<Vec<Schedule>, String>;
    async fn list_for_workflow(&self, workflow_id: &str) -> Result<Vec<Schedule>, String>;
    async fn create(&self, payload: CreateSchedule) -> Result<Schedule, String>;
    async fn toggle(&self, id: &str, enabled: bool) -> Result<Schedule, String>;
    async fn delete(&self, id: &str) -> Result<(), String>;
}

#[async_trait]
pub trait TaskRepo: Send + Sync {
    async fn list(&self) -> Result<Vec<Task>, String>;
    async fn get(&self, id: &str) -> Result<Option<Task>, String>;
    async fn create(&self, payload: CreateTask) -> Result<Task, String>;
    async fn update(&self, id: &str, payload: UpdateTask) -> Result<Task, String>;
    async fn delete(&self, id: &str) -> Result<(), String>;
}

#[async_trait]
pub trait UserRepo: Send + Sync {
    async fn list(&self) -> Result<Vec<User>, String>;
    async fn create(&self, name: String) -> Result<User, String>;
    async fn exists(&self, id: &str) -> Result<bool, String>;
}
