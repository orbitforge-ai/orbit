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
use crate::models::bus::{
    BusMessage, BusSubscription, BusThreadMessage, CreateBusSubscription, PaginatedBusThread,
};
use crate::models::chat::{
    ChatMessageRows, ChatSession, ChatSessionMeta, ChatSessionTokenUsage, MessageReactionRow,
    SessionExecutionStatus,
};
use crate::models::project::{
    CreateProject, Project, ProjectAgent, ProjectAgentWithMeta, ProjectSummary, UpdateProject,
};
use crate::models::project_board::ProjectBoard;
use crate::models::project_board_column::ProjectBoardColumn;
use crate::models::project_workflow::ProjectWorkflow;
use crate::models::run::{Run, RunSummary};
use crate::models::schedule::{CreateSchedule, Schedule};
use crate::models::task::{CreateTask, Task, UpdateTask};
use crate::models::user::User;
use crate::models::work_item::WorkItem;
use crate::models::work_item_comment::WorkItemComment;
use crate::models::work_item_event::WorkItemEvent;
use crate::models::workflow_run::WorkflowRun;

/// Top-level repository facade. The concrete impl picks the backend.
pub trait Repos: Send + Sync {
    fn agents(&self) -> &dyn AgentRepo;
    fn bus_messages(&self) -> &dyn BusMessageRepo;
    fn bus_subscriptions(&self) -> &dyn BusSubscriptionRepo;
    fn chat(&self) -> &dyn ChatRepo;
    fn project_board_columns(&self) -> &dyn ProjectBoardColumnRepo;
    fn project_boards(&self) -> &dyn ProjectBoardRepo;
    fn project_workflows(&self) -> &dyn ProjectWorkflowRepo;
    fn projects(&self) -> &dyn ProjectRepo;
    fn runs(&self) -> &dyn RunRepo;
    fn schedules(&self) -> &dyn ScheduleRepo;
    fn tasks(&self) -> &dyn TaskRepo;
    fn users(&self) -> &dyn UserRepo;
    fn work_items(&self) -> &dyn WorkItemRepo;
    fn work_item_events(&self) -> &dyn WorkItemEventRepo;
    fn workflow_runs(&self) -> &dyn WorkflowRunRepo;
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

    // ── Membership reads (project_agents join table) ────────────────────────
    /// Agents that belong to a project, in creation order.
    async fn list_agents(&self, project_id: &str) -> Result<Vec<Agent>, String>;
    /// Same as `list_agents` but each row carries the `is_default` flag from
    /// the membership row, so the UI can highlight the per-project default.
    async fn list_agents_with_meta(
        &self,
        project_id: &str,
    ) -> Result<Vec<ProjectAgentWithMeta>, String>;
    /// All projects an agent is a member of, in `added_at` order.
    async fn list_for_agent(&self, agent_id: &str) -> Result<Vec<Project>, String>;
    /// True when the project membership row exists.
    async fn agent_in_project(&self, project_id: &str, agent_id: &str) -> Result<bool, String>;

    /// Inserts (or updates) the membership row. `INSERT OR REPLACE` semantics
    /// so toggling the default flag for an existing member is a single call.
    async fn add_agent(
        &self,
        project_id: &str,
        agent_id: &str,
        is_default: bool,
    ) -> Result<ProjectAgent, String>;
    /// Removes the membership row and clears any work-item assignments held
    /// by this agent in this project. Cards stay in their column — a new
    /// claimant is needed for work to continue.
    async fn remove_agent(&self, project_id: &str, agent_id: &str) -> Result<(), String>;
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

/// Read-only filter for `RunRepo::list`.
#[derive(Default, Clone, Debug)]
pub struct RunListFilter {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub task_id: Option<String>,
    /// "all" or `None` means no state filter.
    pub state_filter: Option<String>,
    pub project_id: Option<String>,
}

/// Run aggregate. Read-only at the repo surface for now — write paths still
/// live in `executor::engine` because they're entangled with process spawning,
/// log file paths, and metadata mutation. Once the executor is decoupled,
/// `start`/`finish`/`update_state` will land here too.
#[async_trait]
pub trait RunRepo: Send + Sync {
    async fn list(&self, filter: RunListFilter) -> Result<Vec<RunSummary>, String>;
    async fn get(&self, id: &str) -> Result<Option<Run>, String>;
    async fn list_active(&self) -> Result<Vec<RunSummary>, String>;
    async fn list_sub_agents(&self, parent_run_id: &str) -> Result<Vec<RunSummary>, String>;
    async fn agent_conversation(&self, run_id: &str) -> Result<Option<serde_json::Value>, String>;
    /// Returns the on-disk log file path for a run, if any.
    async fn log_path(&self, run_id: &str) -> Result<Option<String>, String>;
    /// Marks an in-flight run as cancelled and stamps `finished_at`. No-op if
    /// the run has already terminated. The actual process kill is handled by
    /// the executor — this just persists the state transition.
    async fn cancel(&self, run_id: &str) -> Result<(), String>;
}

/// Project board columns (kanban lanes inside a board). Read-only at the
/// trait surface — writes are entangled with default-column promotion,
/// optimistic-concurrency revision checks, and cross-table re-parenting on
/// delete, so they stay as Tauri command bodies in
/// `commands/project_board_columns.rs`. The legacy `*_sync` helpers there
/// are still called from inside transactions in other modules and stay too.
#[async_trait]
pub trait ProjectBoardColumnRepo: Send + Sync {
    /// Lists columns for a project, optionally narrowed to a single board.
    /// When `board_id` is None, falls back to the project's default board.
    async fn list(
        &self,
        project_id: &str,
        board_id: Option<String>,
    ) -> Result<Vec<ProjectBoardColumn>, String>;
    async fn get(&self, id: &str) -> Result<Option<ProjectBoardColumn>, String>;
}

/// Project workflows (the visual graph editor's workflow definitions).
/// Read-only at the trait surface — write paths involve graph normalization,
/// trigger reconciliation, and a transactional graph swap; they stay as
/// `*_with_db` helpers in `commands/project_workflows.rs` for now.
#[async_trait]
pub trait ProjectWorkflowRepo: Send + Sync {
    async fn list(&self, project_id: &str, limit: i64) -> Result<Vec<ProjectWorkflow>, String>;
    async fn get(&self, id: &str) -> Result<ProjectWorkflow, String>;
    /// Project-id of the project that owns the given workflow.
    async fn lookup_project_id(&self, workflow_id: &str) -> Result<String, String>;
    /// Joined `(workflow_id, project_id)` for a `workflow_runs` row — the
    /// run-loop dispatcher uses this to scope event emission.
    async fn lookup_run_scope(&self, run_id: &str) -> Result<(String, String), String>;
}

/// Work items aggregate (kanban cards). Read-only at the trait surface
/// because the write paths are entangled with `work_item_events` insertion
/// across tables and are also called from agent tools as
/// `*_with_db` helpers in `commands/work_items.rs`. Once the executor
/// switches to the trait surface, mutations land here too.
#[async_trait]
pub trait WorkItemRepo: Send + Sync {
    async fn list(
        &self,
        project_id: &str,
        board_id: Option<String>,
    ) -> Result<Vec<WorkItem>, String>;
    async fn get(&self, id: &str) -> Result<WorkItem, String>;
    async fn list_comments(&self, work_item_id: &str) -> Result<Vec<WorkItemComment>, String>;
}

/// Work item events. List is the only command-surface call; appending events
/// happens inside larger transactions in `commands/work_items.rs` so callers
/// keep using `insert_event` against a raw `&Connection` for now.
#[async_trait]
pub trait WorkItemEventRepo: Send + Sync {
    async fn list(&self, work_item_id: &str) -> Result<Vec<WorkItemEvent>, String>;
}

/// Workflow runs. Wraps the existing `workflows::orchestrator` / `store`
/// helpers so command code no longer reaches into them with a `DbPool`.
/// Write paths (start/cancel) still live in the orchestrator since they
/// also drive the run loop.
#[async_trait]
pub trait WorkflowRunRepo: Send + Sync {
    async fn list_for_workflow(
        &self,
        workflow_id: &str,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>, String>;
    async fn list_for_project(
        &self,
        project_id: &str,
        limit: i64,
    ) -> Result<Vec<crate::models::workflow_run::WorkflowRunSummary>, String>;
    async fn get_with_steps(
        &self,
        run_id: &str,
    ) -> Result<crate::models::workflow_run::WorkflowRunWithSteps, String>;
    async fn cancel(&self, run_id: &str) -> Result<(), String>;
}

/// Filter knobs for `ChatRepo::list_sessions`.
#[derive(Default, Clone, Debug)]
pub struct ChatSessionListFilter {
    pub agent_id: String,
    pub include_archived: bool,
    /// Empty vec means "all session types".
    pub session_types: Vec<String>,
    pub project_id: Option<String>,
}

/// Chat aggregate. Read-only at the repo trait surface — write paths live in
/// `commands/chat.rs` because they're entangled with the streaming executor,
/// session-execution registry, and worktree lifecycle. Once that machinery
/// has its own boundary, mutations land here too.
#[async_trait]
pub trait ChatRepo: Send + Sync {
    async fn list_sessions(
        &self,
        filter: ChatSessionListFilter,
    ) -> Result<Vec<ChatSession>, String>;

    /// Returns paginated message rows. `limit = 0` means "all messages".
    /// The repo returns the raw stored content JSON; the caller decodes it
    /// into the typed `ContentBlock` shape the UI uses.
    async fn get_messages(
        &self,
        session_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<ChatMessageRows, String>;

    async fn session_meta(&self, session_id: &str) -> Result<ChatSessionMeta, String>;
    async fn session_execution(&self, session_id: &str) -> Result<SessionExecutionStatus, String>;
    /// Convenience lookup used by `cancel_agent_session` to reject cancels
    /// against session types that don't run an agent loop.
    async fn session_type(&self, session_id: &str) -> Result<String, String>;
    async fn list_message_reactions(
        &self,
        session_id: &str,
    ) -> Result<Vec<MessageReactionRow>, String>;
    /// Last-input-tokens + agent_id, joined so the UI can compute the
    /// remaining-context-window percentage in one round-trip.
    async fn token_usage(&self, session_id: &str) -> Result<ChatSessionTokenUsage, String>;
}

/// Inter-agent message bus — read surface only at the repo trait. The fan-out
/// dispatcher in `triggers/dispatcher.rs` still inserts messages directly via
/// `&Connection` because it does so inside a larger transaction that also
/// updates subscription state.
#[async_trait]
pub trait BusMessageRepo: Send + Sync {
    /// Lists raw bus messages, optionally filtered to those involving `agent_id`
    /// (either as sender or recipient).
    async fn list(
        &self,
        agent_id: Option<String>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<BusMessage>, String>;
    /// Returns the inbox thread for `agent_id` joined with the run/session
    /// state of the message that triggered it (so the UI can show "completed",
    /// "queued", etc. inline).
    async fn thread_for_agent(
        &self,
        agent_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<PaginatedBusThread, String>;
}

/// Bus subscription rules. Subscriptions are "when agent X emits event Y, run
/// task Z". Toggle/delete have an optional cloud mirror side-effect; that
/// stays in the command file (cloud upsert is a transport concern).
#[async_trait]
pub trait BusSubscriptionRepo: Send + Sync {
    async fn list(&self, agent_id: Option<String>) -> Result<Vec<BusSubscription>, String>;
    async fn create(&self, payload: CreateBusSubscription) -> Result<BusSubscription, String>;
    async fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String>;
    async fn delete(&self, id: &str) -> Result<(), String>;
}

/// Project boards. Boards are kanban surfaces inside a project; one board
/// is the project default. Delete has cross-table side-effects (re-parents
/// columns + work items) so it stays here for atomicity.
#[async_trait]
pub trait ProjectBoardRepo: Send + Sync {
    async fn list(&self, project_id: &str) -> Result<Vec<ProjectBoard>, String>;
    async fn get(&self, id: &str) -> Result<Option<ProjectBoard>, String>;
    async fn create(
        &self,
        payload: crate::models::project_board::CreateProjectBoard,
    ) -> Result<ProjectBoard, String>;
    async fn update(
        &self,
        id: &str,
        payload: crate::models::project_board::UpdateProjectBoard,
    ) -> Result<ProjectBoard, String>;
    async fn delete(
        &self,
        id: &str,
        payload: crate::models::project_board::DeleteProjectBoard,
    ) -> Result<(), String>;
}
