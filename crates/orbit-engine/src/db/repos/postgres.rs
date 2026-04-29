//! Postgres-backed `Repos` impl built on `sqlx::PgPool`.
//!
//! This backend mirrors the SQLite repo trait surface and always scopes rows
//! through `RepoCtx.tenant_id`. Queries intentionally use runtime-checked
//! `sqlx::query` calls instead of macros so the crate can compile without a
//! live database URL during local development and CI.

use async_trait::async_trait;
use sqlx::postgres::{PgPool, PgRow};
use sqlx::{QueryBuilder, Row};
use ulid::Ulid;

use crate::db::repos::{
    AgentRepo, BusMessageRepo, BusSubscriptionRepo, ChatRepo, ChatSessionListFilter,
    ProjectBoardColumnRepo, ProjectBoardRepo, ProjectRepo, ProjectWorkflowRepo, RepoCtx, Repos,
    RunListFilter, RunRepo, ScheduleRepo, TaskRepo, UserRepo, WorkItemEventRepo, WorkItemRepo,
    WorkflowRunRepo, WorkflowSeenItemRepo,
};
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
    CreateProjectWorkflow, ProjectWorkflow, UpdateProjectWorkflow,
};
use crate::models::run::{Run, RunSummary};
use crate::models::schedule::{CreateSchedule, RecurringConfig, Schedule};
use crate::models::task::{CreateTask, Task, UpdateTask};
use crate::models::user::User;
use crate::models::work_item::{CreateWorkItem, UpdateWorkItem, WorkItem};
use crate::models::work_item_comment::{CommentAuthor, WorkItemComment};
use crate::models::work_item_event::WorkItemEvent;
use crate::models::workflow_run::{
    WorkflowRun, WorkflowRunStep, WorkflowRunSummary, WorkflowRunWithSteps,
};
use crate::scheduler::converter::next_n_runs;

#[derive(Clone)]
pub struct PgRepos {
    pool: PgPool,
    ctx: RepoCtx,
}

impl PgRepos {
    pub fn new(pool: PgPool) -> Self {
        Self::with_ctx(pool, RepoCtx::default())
    }

    pub fn with_tenant(pool: PgPool, tenant_id: impl Into<String>) -> Self {
        Self::with_ctx(pool, RepoCtx::new(tenant_id))
    }

    pub fn with_ctx(pool: PgPool, ctx: RepoCtx) -> Self {
        Self { pool, ctx }
    }

    fn tenant_id(&self) -> String {
        self.ctx.tenant_id.clone()
    }
}

#[async_trait]
impl WorkflowSeenItemRepo for PgRepos {
    async fn filter_unseen(
        &self,
        workflow_id: &str,
        node_id: &str,
        source_key: &str,
        fingerprints: &[String],
    ) -> Result<Vec<bool>, String> {
        let tenant_id = self.tenant_id();
        let now = now();
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let mut inserted = Vec::with_capacity(fingerprints.len());
        for fingerprint in fingerprints {
            let result = sqlx::query(
                "INSERT INTO workflow_seen_items (
                    id, workflow_id, node_id, source_key, fingerprint, created_at, tenant_id
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7)
                 ON CONFLICT (workflow_id, node_id, source_key, fingerprint) DO NOTHING",
            )
            .bind(Ulid::new().to_string())
            .bind(workflow_id)
            .bind(node_id)
            .bind(source_key)
            .bind(fingerprint)
            .bind(&now)
            .bind(&tenant_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
            inserted.push(result.rows_affected() == 1);
        }
        tx.commit().await.map_err(db_err)?;
        Ok(inserted)
    }
}

impl Repos for PgRepos {
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
    fn workflow_seen_items(&self) -> &dyn WorkflowSeenItemRepo {
        self
    }
}

fn db_err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn json_string(value: &serde_json::Value) -> Result<String, String> {
    serde_json::to_string(value).map_err(db_err)
}

fn parse_json(raw: &str) -> serde_json::Value {
    serde_json::from_str(raw).unwrap_or(serde_json::Value::Null)
}

fn parse_json_array<T: serde::de::DeserializeOwned>(raw: &str) -> Vec<T> {
    serde_json::from_str(raw).unwrap_or_default()
}

fn bool_row(row: &PgRow, idx: usize) -> Result<bool, sqlx::Error> {
    row.try_get(idx)
}

const TASK_COLUMNS: &str = "id, name, description, kind, config::text, max_duration_seconds, max_retries,
    retry_delay_seconds, concurrency_policy, tags::text, agent_id, enabled, created_at::text, updated_at::text, project_id";

fn map_task_row(row: &PgRow) -> Result<Task, sqlx::Error> {
    let config: String = row.try_get(4)?;
    let tags: String = row.try_get(9)?;
    Ok(Task {
        id: row.try_get(0)?,
        name: row.try_get(1)?,
        description: row.try_get(2)?,
        kind: row.try_get(3)?,
        config: parse_json(&config),
        max_duration_seconds: row.try_get(5)?,
        max_retries: row.try_get(6)?,
        retry_delay_seconds: row.try_get(7)?,
        concurrency_policy: row.try_get(8)?,
        tags: parse_json_array(&tags),
        agent_id: row.try_get(10)?,
        enabled: bool_row(row, 11)?,
        created_at: row.try_get(12)?,
        updated_at: row.try_get(13)?,
        project_id: row.try_get(14)?,
    })
}

const AGENT_COLUMNS: &str =
    "id, name, description, state, max_concurrent_runs, heartbeat_at::text, created_at::text, updated_at::text";

fn map_agent_row(row: &PgRow) -> Result<Agent, sqlx::Error> {
    Ok(Agent {
        id: row.try_get(0)?,
        name: row.try_get(1)?,
        description: row.try_get(2)?,
        state: row.try_get(3)?,
        max_concurrent_runs: row.try_get(4)?,
        heartbeat_at: row.try_get(5)?,
        created_at: row.try_get(6)?,
        updated_at: row.try_get(7)?,
    })
}

fn map_project_row(row: &PgRow) -> Result<Project, sqlx::Error> {
    Ok(Project {
        id: row.try_get(0)?,
        name: row.try_get(1)?,
        description: row.try_get(2)?,
        created_at: row.try_get(3)?,
        updated_at: row.try_get(4)?,
    })
}

fn map_project_summary_row(row: &PgRow) -> Result<ProjectSummary, sqlx::Error> {
    Ok(ProjectSummary {
        id: row.try_get(0)?,
        name: row.try_get(1)?,
        description: row.try_get(2)?,
        created_at: row.try_get(3)?,
        updated_at: row.try_get(4)?,
        agent_count: row.try_get(5)?,
    })
}

const SCHEDULE_COLUMNS: &str = "id, task_id, workflow_id, target_kind, kind, config::text, enabled,
    next_run_at::text, last_run_at::text, created_at::text, updated_at::text";

fn map_schedule_row(row: &PgRow) -> Result<Schedule, sqlx::Error> {
    let config: String = row.try_get(5)?;
    Ok(Schedule {
        id: row.try_get(0)?,
        task_id: row.try_get(1)?,
        workflow_id: row.try_get(2)?,
        target_kind: row.try_get(3)?,
        kind: row.try_get(4)?,
        config: parse_json(&config),
        enabled: bool_row(row, 6)?,
        next_run_at: row.try_get(7)?,
        last_run_at: row.try_get(8)?,
        created_at: row.try_get(9)?,
        updated_at: row.try_get(10)?,
    })
}

fn map_user_row(row: &PgRow) -> Result<User, sqlx::Error> {
    Ok(User {
        id: row.try_get(0)?,
        name: row.try_get(1)?,
        is_default: bool_row(row, 2)?,
        created_at: row.try_get(3)?,
    })
}

const PROJECT_BOARD_COLUMNS: &str =
    "id, project_id, name, prefix, position, is_default, created_at::text, updated_at::text";

fn map_project_board_row(row: &PgRow) -> Result<ProjectBoard, sqlx::Error> {
    Ok(ProjectBoard {
        id: row.try_get(0)?,
        project_id: row.try_get(1)?,
        name: row.try_get(2)?,
        prefix: row.try_get(3)?,
        position: row.try_get(4)?,
        is_default: bool_row(row, 5)?,
        created_at: row.try_get(6)?,
        updated_at: row.try_get(7)?,
    })
}

const PROJECT_BOARD_COLUMN_COLUMNS: &str =
    "id, project_id, board_id, name, role, is_default, position, created_at::text, updated_at::text";

fn map_project_board_column_row(row: &PgRow) -> Result<ProjectBoardColumn, sqlx::Error> {
    Ok(ProjectBoardColumn {
        id: row.try_get(0)?,
        project_id: row.try_get(1)?,
        board_id: row.try_get::<Option<String>, _>(2)?.unwrap_or_default(),
        name: row.try_get(3)?,
        role: row.try_get(4)?,
        is_default: bool_row(row, 5)?,
        position: row.try_get(6)?,
        created_at: row.try_get(7)?,
        updated_at: row.try_get(8)?,
    })
}

const BUS_MESSAGE_COLUMNS: &str = "id, from_agent_id, from_run_id, from_session_id, to_agent_id,
    to_run_id, to_session_id, kind, event_type, payload::text, status, created_at::text";

fn map_bus_message_row(row: &PgRow) -> Result<BusMessage, sqlx::Error> {
    let payload: String = row.try_get(9)?;
    Ok(BusMessage {
        id: row.try_get(0)?,
        from_agent_id: row.try_get(1)?,
        from_run_id: row.try_get(2)?,
        from_session_id: row.try_get(3)?,
        to_agent_id: row.try_get(4)?,
        to_run_id: row.try_get(5)?,
        to_session_id: row.try_get(6)?,
        kind: row.try_get(7)?,
        event_type: row.try_get(8)?,
        payload: parse_json(&payload),
        status: row.try_get(10)?,
        created_at: row.try_get(11)?,
    })
}

const BUS_SUBSCRIPTION_COLUMNS: &str = "id, subscriber_agent_id, source_agent_id, event_type,
    task_id, payload_template, enabled, max_chain_depth, created_at::text, updated_at::text";

fn map_bus_subscription_row(row: &PgRow) -> Result<BusSubscription, sqlx::Error> {
    Ok(BusSubscription {
        id: row.try_get(0)?,
        subscriber_agent_id: row.try_get(1)?,
        source_agent_id: row.try_get(2)?,
        event_type: row.try_get(3)?,
        task_id: row.try_get(4)?,
        payload_template: row.try_get(5)?,
        enabled: bool_row(row, 6)?,
        max_chain_depth: row.try_get(7)?,
        created_at: row.try_get(8)?,
        updated_at: row.try_get(9)?,
    })
}

const CHAT_SESSION_SELECT: &str =
    "SELECT cs.id, cs.agent_id, cs.title, cs.archived, cs.session_type,
    cs.parent_session_id, cs.source_bus_message_id, cs.chain_depth, cs.execution_state,
    cs.finish_summary, cs.terminal_error, bm.from_agent_id, a.name, src.id, src.title,
    cs.created_at::text, cs.updated_at::text, cs.project_id, cs.worktree_name, cs.worktree_branch,
    cs.worktree_path
    FROM chat_sessions cs
    LEFT JOIN bus_messages bm ON bm.id = cs.source_bus_message_id AND bm.tenant_id = cs.tenant_id
    LEFT JOIN agents a ON a.id = bm.from_agent_id AND a.tenant_id = cs.tenant_id
    LEFT JOIN chat_sessions src ON src.id = bm.from_session_id AND src.tenant_id = cs.tenant_id";

fn map_chat_session_row(row: &PgRow) -> Result<ChatSession, sqlx::Error> {
    Ok(ChatSession {
        id: row.try_get(0)?,
        agent_id: row.try_get(1)?,
        title: row.try_get(2)?,
        archived: bool_row(row, 3)?,
        session_type: row.try_get(4)?,
        parent_session_id: row.try_get(5)?,
        source_bus_message_id: row.try_get(6)?,
        chain_depth: row.try_get(7)?,
        execution_state: row.try_get(8)?,
        finish_summary: row.try_get(9)?,
        terminal_error: row.try_get(10)?,
        source_agent_id: row.try_get(11)?,
        source_agent_name: row.try_get(12)?,
        source_session_id: row.try_get(13)?,
        source_session_title: row.try_get(14)?,
        created_at: row.try_get(15)?,
        updated_at: row.try_get(16)?,
        project_id: row.try_get(17)?,
        worktree_name: row.try_get(18)?,
        worktree_branch: row.try_get(19)?,
        worktree_path: row.try_get(20)?,
    })
}

fn map_chat_message_row(row: &PgRow) -> Result<ChatMessageRow, sqlx::Error> {
    Ok(ChatMessageRow {
        id: row.try_get(0)?,
        role: row.try_get(1)?,
        content_json: row.try_get(2)?,
        created_at: row.try_get(3)?,
        is_compacted: bool_row(row, 4)?,
    })
}

const WORK_ITEM_COLUMNS: &str = "id, project_id, board_id, title, description, kind, column_id,
    status, priority, assignee_agent_id, created_by_agent_id, parent_work_item_id, position,
    labels::text, metadata::text, blocked_reason, started_at::text, completed_at::text,
    created_at::text, updated_at::text";

fn map_work_item_row(row: &PgRow) -> Result<WorkItem, sqlx::Error> {
    let labels: String = row.try_get(13)?;
    let metadata: String = row.try_get(14)?;
    Ok(WorkItem {
        id: row.try_get(0)?,
        project_id: row.try_get(1)?,
        board_id: row.try_get(2)?,
        title: row.try_get(3)?,
        description: row.try_get(4)?,
        kind: row.try_get(5)?,
        column_id: row.try_get(6)?,
        status: row.try_get(7)?,
        priority: row.try_get(8)?,
        assignee_agent_id: row.try_get(9)?,
        created_by_agent_id: row.try_get(10)?,
        parent_work_item_id: row.try_get(11)?,
        position: row.try_get(12)?,
        labels: parse_json_array(&labels),
        metadata: parse_json(&metadata),
        blocked_reason: row.try_get(15)?,
        started_at: row.try_get(16)?,
        completed_at: row.try_get(17)?,
        created_at: row.try_get(18)?,
        updated_at: row.try_get(19)?,
    })
}

const WORK_ITEM_COMMENT_COLUMNS: &str =
    "id, work_item_id, author_kind, author_agent_id, body, created_at::text, updated_at::text";

fn map_work_item_comment_row(row: &PgRow) -> Result<WorkItemComment, sqlx::Error> {
    Ok(WorkItemComment {
        id: row.try_get(0)?,
        work_item_id: row.try_get(1)?,
        author_kind: row.try_get(2)?,
        author_agent_id: row.try_get(3)?,
        body: row.try_get(4)?,
        created_at: row.try_get(5)?,
        updated_at: row.try_get(6)?,
    })
}

const WORK_ITEM_EVENT_COLUMNS: &str =
    "id, work_item_id, actor_kind, actor_agent_id, kind, payload_json::text, created_at::text";

fn map_work_item_event_row(row: &PgRow) -> Result<WorkItemEvent, sqlx::Error> {
    let payload: String = row.try_get(5)?;
    Ok(WorkItemEvent {
        id: row.try_get(0)?,
        work_item_id: row.try_get(1)?,
        actor_kind: row.try_get(2)?,
        actor_agent_id: row.try_get(3)?,
        kind: row.try_get(4)?,
        payload: parse_json(&payload),
        created_at: row.try_get(6)?,
    })
}

const PROJECT_WORKFLOW_COLUMNS: &str = "id, project_id, name, description, enabled, graph::text,
    trigger_kind, trigger_config::text, version, created_at::text, updated_at::text";

fn map_project_workflow_row(row: &PgRow) -> Result<ProjectWorkflow, sqlx::Error> {
    let graph: String = row.try_get(5)?;
    let trigger_config: String = row.try_get(7)?;
    Ok(ProjectWorkflow {
        id: row.try_get(0)?,
        project_id: row.try_get(1)?,
        name: row.try_get(2)?,
        description: row.try_get(3)?,
        enabled: bool_row(row, 4)?,
        graph: serde_json::from_str(&graph).unwrap_or_default(),
        trigger_kind: row.try_get(6)?,
        trigger_config: parse_json(&trigger_config),
        version: row.try_get(8)?,
        created_at: row.try_get(9)?,
        updated_at: row.try_get(10)?,
    })
}

const WORKFLOW_RUN_COLUMNS: &str = "id, workflow_id, workflow_version, graph_snapshot::text,
    trigger_kind, trigger_data::text, status, error, started_at::text, completed_at::text,
    created_at::text";

fn map_workflow_run_row(row: &PgRow) -> Result<WorkflowRun, sqlx::Error> {
    let graph: String = row.try_get(3)?;
    let trigger_data: String = row.try_get(5)?;
    Ok(WorkflowRun {
        id: row.try_get(0)?,
        workflow_id: row.try_get(1)?,
        workflow_version: row.try_get(2)?,
        graph_snapshot: parse_json(&graph),
        trigger_kind: row.try_get(4)?,
        trigger_data: parse_json(&trigger_data),
        status: row.try_get(6)?,
        error: row.try_get(7)?,
        started_at: row.try_get(8)?,
        completed_at: row.try_get(9)?,
        created_at: row.try_get(10)?,
    })
}

fn validate_board_prefix(prefix: &str) -> Result<(), String> {
    let trimmed = prefix.trim();
    if trimmed.len() < 2 || trimmed.len() > 8 {
        return Err("board prefix must be 2 to 8 characters long".into());
    }
    if !trimmed.chars().all(|c| c.is_ascii_uppercase()) {
        return Err("board prefix must contain only uppercase letters A-Z".into());
    }
    Ok(())
}

async fn fetch_task(pool: &PgPool, id: &str, tenant_id: &str) -> Result<Task, String> {
    sqlx::query(&format!(
        "SELECT {TASK_COLUMNS} FROM tasks WHERE id = $1 AND tenant_id = $2"
    ))
    .bind(id)
    .bind(tenant_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)
    .and_then(|row| map_task_row(&row).map_err(db_err))
}

async fn fetch_schedule(pool: &PgPool, id: &str, tenant_id: &str) -> Result<Schedule, String> {
    sqlx::query(&format!(
        "SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE id = $1 AND tenant_id = $2"
    ))
    .bind(id)
    .bind(tenant_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)
    .and_then(|row| map_schedule_row(&row).map_err(db_err))
}

async fn fetch_work_item(pool: &PgPool, id: &str, tenant_id: &str) -> Result<WorkItem, String> {
    sqlx::query(&format!(
        "SELECT {WORK_ITEM_COLUMNS} FROM work_items WHERE id = $1 AND tenant_id = $2"
    ))
    .bind(id)
    .bind(tenant_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)
    .and_then(|row| map_work_item_row(&row).map_err(db_err))
}

async fn insert_work_item_event(
    pool: &PgPool,
    tenant_id: &str,
    work_item_id: &str,
    actor_kind: &str,
    actor_agent_id: Option<&str>,
    kind: &str,
    payload: serde_json::Value,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO work_item_events
         (id, work_item_id, actor_kind, actor_agent_id, kind, payload_json, created_at, tenant_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(Ulid::new().to_string())
    .bind(work_item_id)
    .bind(actor_kind)
    .bind(actor_agent_id)
    .bind(kind)
    .bind(json_string(&payload)?)
    .bind(now())
    .bind(tenant_id)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

#[async_trait]
impl TaskRepo for PgRepos {
    async fn list(&self) -> Result<Vec<Task>, String> {
        let rows = sqlx::query(&format!(
            "SELECT {TASK_COLUMNS} FROM tasks WHERE tenant_id = $1 ORDER BY created_at DESC"
        ))
        .bind(self.tenant_id())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.iter()
            .map(map_task_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn get(&self, id: &str) -> Result<Option<Task>, String> {
        let row = sqlx::query(&format!(
            "SELECT {TASK_COLUMNS} FROM tasks WHERE id = $1 AND tenant_id = $2"
        ))
        .bind(id)
        .bind(self.tenant_id())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        row.map(|row| map_task_row(&row).map_err(db_err))
            .transpose()
    }

    async fn create(&self, payload: CreateTask) -> Result<Task, String> {
        let tenant_id = self.tenant_id();
        let id = Ulid::new().to_string();
        let now = now();
        let config = json_string(&payload.config)?;
        let tags = serde_json::to_string(&payload.tags.unwrap_or_default()).map_err(db_err)?;
        let agent_id = payload.agent_id.unwrap_or_else(|| "default".to_string());
        sqlx::query(
            "INSERT INTO tasks (id, name, description, kind, config, max_duration_seconds,
             max_retries, retry_delay_seconds, concurrency_policy, tags, agent_id, enabled,
             created_at, updated_at, project_id, tenant_id)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,true,$12,$12,$13,$14)",
        )
        .bind(&id)
        .bind(payload.name)
        .bind(payload.description)
        .bind(payload.kind)
        .bind(config)
        .bind(payload.max_duration_seconds.unwrap_or(3600))
        .bind(payload.max_retries.unwrap_or(0))
        .bind(payload.retry_delay_seconds.unwrap_or(60))
        .bind(
            payload
                .concurrency_policy
                .unwrap_or_else(|| "allow".to_string()),
        )
        .bind(tags)
        .bind(agent_id)
        .bind(now)
        .bind(payload.project_id)
        .bind(&tenant_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        fetch_task(&self.pool, &id, &tenant_id).await
    }

    async fn update(&self, id: &str, payload: UpdateTask) -> Result<Task, String> {
        let tenant_id = self.tenant_id();
        let now = now();
        if let Some(v) = payload.name {
            sqlx::query(
                "UPDATE tasks SET name = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4",
            )
            .bind(v)
            .bind(&now)
            .bind(id)
            .bind(&tenant_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        }
        if let Some(v) = payload.description {
            sqlx::query("UPDATE tasks SET description = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(v).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        if let Some(v) = payload.config {
            sqlx::query(
                "UPDATE tasks SET config = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4",
            )
            .bind(json_string(&v)?)
            .bind(&now)
            .bind(id)
            .bind(&tenant_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        }
        if let Some(v) = payload.max_duration_seconds {
            sqlx::query("UPDATE tasks SET max_duration_seconds = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(v).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        if let Some(v) = payload.max_retries {
            sqlx::query("UPDATE tasks SET max_retries = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(v).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        if let Some(v) = payload.retry_delay_seconds {
            sqlx::query("UPDATE tasks SET retry_delay_seconds = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(v).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        if let Some(v) = payload.concurrency_policy {
            sqlx::query("UPDATE tasks SET concurrency_policy = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(v).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        if let Some(v) = payload.tags {
            sqlx::query(
                "UPDATE tasks SET tags = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4",
            )
            .bind(serde_json::to_string(&v).map_err(db_err)?)
            .bind(&now)
            .bind(id)
            .bind(&tenant_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        }
        if let Some(v) = payload.agent_id {
            sqlx::query(
                "UPDATE tasks SET agent_id = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4",
            )
            .bind(v)
            .bind(&now)
            .bind(id)
            .bind(&tenant_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        }
        if let Some(v) = payload.enabled {
            sqlx::query(
                "UPDATE tasks SET enabled = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4",
            )
            .bind(v)
            .bind(&now)
            .bind(id)
            .bind(&tenant_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        }
        if let Some(v) = payload.project_id {
            let project_id = if v.is_empty() { None } else { Some(v) };
            sqlx::query("UPDATE tasks SET project_id = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(project_id).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        fetch_task(&self.pool, id, &tenant_id).await
    }

    async fn delete(&self, id: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM tasks WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(self.tenant_id())
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

#[async_trait]
impl AgentRepo for PgRepos {
    async fn list(&self) -> Result<Vec<Agent>, String> {
        let rows = sqlx::query(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents WHERE tenant_id = $1 ORDER BY created_at ASC"
        ))
        .bind(self.tenant_id())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.iter()
            .map(map_agent_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn get(&self, id: &str) -> Result<Option<Agent>, String> {
        let row = sqlx::query(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents WHERE id = $1 AND tenant_id = $2"
        ))
        .bind(id)
        .bind(self.tenant_id())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        row.map(|row| map_agent_row(&row).map_err(db_err))
            .transpose()
    }

    async fn create_basic(&self, payload: CreateAgent) -> Result<Agent, String> {
        let tenant_id = self.tenant_id();
        let id = self.next_available_id(&payload.name, None).await?;
        let now = now();
        sqlx::query(
            "INSERT INTO agents (id, name, description, state, max_concurrent_runs, created_at, updated_at, tenant_id)
             VALUES ($1,$2,$3,'idle',$4,$5,$5,$6)",
        )
        .bind(&id)
        .bind(payload.name)
        .bind(payload.description)
        .bind(payload.max_concurrent_runs.unwrap_or(5))
        .bind(now)
        .bind(&tenant_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        AgentRepo::get(self, &id)
            .await?
            .ok_or_else(|| format!("agent not found after insert: {id}"))
    }

    async fn set_model_config(&self, id: &str, model_config_json: &str) -> Result<(), String> {
        sqlx::query("UPDATE agents SET model_config = $1 WHERE id = $2 AND tenant_id = $3")
            .bind(model_config_json)
            .bind(id)
            .bind(self.tenant_id())
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn update_basic(&self, id: &str, payload: UpdateAgent) -> Result<Agent, String> {
        let tenant_id = self.tenant_id();
        let now = now();
        if let Some(v) = payload.name {
            sqlx::query(
                "UPDATE agents SET name = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4",
            )
            .bind(v)
            .bind(&now)
            .bind(id)
            .bind(&tenant_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        }
        if let Some(v) = payload.description {
            sqlx::query("UPDATE agents SET description = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(v).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        if let Some(v) = payload.max_concurrent_runs {
            sqlx::query("UPDATE agents SET max_concurrent_runs = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(v).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        AgentRepo::get(self, id)
            .await?
            .ok_or_else(|| format!("agent not found: {id}"))
    }

    async fn delete(&self, id: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM agents WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(self.tenant_id())
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn next_available_id(
        &self,
        name: &str,
        current_id: Option<&str>,
    ) -> Result<String, String> {
        let base = workspace::slugify(name);
        let base = if base.is_empty() {
            "agent".to_string()
        } else {
            base
        };
        let tenant_id = self.tenant_id();
        let mut candidate = base.clone();
        let mut suffix = 1;
        loop {
            let existing: Option<String> =
                sqlx::query_scalar("SELECT id FROM agents WHERE id = $1 AND tenant_id = $2")
                    .bind(&candidate)
                    .bind(&tenant_id)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(db_err)?;
            match existing.as_deref() {
                None => return Ok(candidate),
                Some(existing_id) if Some(existing_id) == current_id => return Ok(candidate),
                Some(_) => {
                    suffix += 1;
                    candidate = format!("{base}-{suffix}");
                }
            }
        }
    }
}

#[async_trait]
impl ProjectRepo for PgRepos {
    async fn list(&self) -> Result<Vec<ProjectSummary>, String> {
        let rows = sqlx::query(
            "SELECT p.id, p.name, p.description, p.created_at::text, p.updated_at::text,
                    COALESCE(pa.agent_count, 0)::bigint AS agent_count
             FROM projects p
             LEFT JOIN (
                 SELECT project_id, COUNT(*)::bigint AS agent_count
                 FROM project_agents
                 WHERE tenant_id = $1
                 GROUP BY project_id
             ) pa ON pa.project_id = p.id
             WHERE p.tenant_id = $1
             ORDER BY p.created_at ASC",
        )
        .bind(self.tenant_id())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.iter()
            .map(map_project_summary_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn get(&self, id: &str) -> Result<Option<Project>, String> {
        let row = sqlx::query(
            "SELECT id, name, description, created_at::text, updated_at::text
             FROM projects WHERE id = $1 AND tenant_id = $2",
        )
        .bind(id)
        .bind(self.tenant_id())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        row.map(|row| map_project_row(&row).map_err(db_err))
            .transpose()
    }

    async fn create_basic(&self, payload: CreateProject) -> Result<Project, String> {
        let tenant_id = self.tenant_id();
        let base = workspace::slugify(&payload.name);
        let base = if base.is_empty() {
            "project".to_string()
        } else {
            base
        };
        let mut candidate = base.clone();
        let mut suffix = 1;
        while sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id = $1 AND tenant_id = $2)",
        )
        .bind(&candidate)
        .bind(&tenant_id)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?
        {
            suffix += 1;
            candidate = format!("{base}-{suffix}");
        }
        let now = now();
        sqlx::query(
            "INSERT INTO projects (id, name, description, created_at, updated_at, tenant_id)
             VALUES ($1,$2,$3,$4,$4,$5)",
        )
        .bind(&candidate)
        .bind(payload.name)
        .bind(payload.description)
        .bind(now)
        .bind(&tenant_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        ProjectRepo::get(self, &candidate)
            .await?
            .ok_or_else(|| format!("project not found after insert: {candidate}"))
    }

    async fn update(&self, id: &str, payload: UpdateProject) -> Result<Project, String> {
        let tenant_id = self.tenant_id();
        let now = now();
        if let Some(v) = payload.name {
            sqlx::query(
                "UPDATE projects SET name = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4",
            )
            .bind(v)
            .bind(&now)
            .bind(id)
            .bind(&tenant_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        }
        if let Some(v) = payload.description {
            sqlx::query("UPDATE projects SET description = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(v).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        ProjectRepo::get(self, id)
            .await?
            .ok_or_else(|| format!("project not found: {id}"))
    }

    async fn delete(&self, id: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM projects WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(self.tenant_id())
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn list_agents(&self, project_id: &str) -> Result<Vec<Agent>, String> {
        let rows = sqlx::query(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents a
             JOIN project_agents pa ON pa.agent_id = a.id AND pa.tenant_id = a.tenant_id
             WHERE pa.project_id = $1 AND pa.tenant_id = $2 AND a.tenant_id = $2
             ORDER BY a.created_at ASC"
        ))
        .bind(project_id)
        .bind(self.tenant_id())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.iter()
            .map(map_agent_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn list_agents_with_meta(
        &self,
        project_id: &str,
    ) -> Result<Vec<ProjectAgentWithMeta>, String> {
        let rows = sqlx::query(&format!(
            "SELECT {AGENT_COLUMNS}, pa.is_default FROM agents a
             JOIN project_agents pa ON pa.agent_id = a.id AND pa.tenant_id = a.tenant_id
             WHERE pa.project_id = $1 AND pa.tenant_id = $2 AND a.tenant_id = $2
             ORDER BY pa.is_default DESC, a.created_at ASC"
        ))
        .bind(project_id)
        .bind(self.tenant_id())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.iter()
            .map(|row| {
                Ok(ProjectAgentWithMeta {
                    agent: map_agent_row(row)?,
                    is_default: row.try_get(8)?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(db_err)
    }

    async fn list_for_agent(&self, agent_id: &str) -> Result<Vec<Project>, String> {
        let rows = sqlx::query(
            "SELECT p.id, p.name, p.description, p.created_at::text, p.updated_at::text
             FROM projects p
             JOIN project_agents pa ON pa.project_id = p.id AND pa.tenant_id = p.tenant_id
             WHERE pa.agent_id = $1 AND pa.tenant_id = $2 AND p.tenant_id = $2
             ORDER BY pa.added_at ASC",
        )
        .bind(agent_id)
        .bind(self.tenant_id())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.iter()
            .map(map_project_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn agent_in_project(&self, project_id: &str, agent_id: &str) -> Result<bool, String> {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM project_agents WHERE project_id = $1 AND agent_id = $2 AND tenant_id = $3)",
        )
        .bind(project_id).bind(agent_id).bind(self.tenant_id()).fetch_one(&self.pool).await.map_err(db_err)
    }

    async fn add_agent(
        &self,
        project_id: &str,
        agent_id: &str,
        is_default: bool,
    ) -> Result<ProjectAgent, String> {
        let added_at = now();
        let tenant_id = self.tenant_id();
        sqlx::query(
            "INSERT INTO project_agents (project_id, agent_id, is_default, added_at, tenant_id)
             VALUES ($1,$2,$3,$4,$5)
             ON CONFLICT(project_id, agent_id) DO UPDATE SET
               is_default = EXCLUDED.is_default,
               added_at = EXCLUDED.added_at,
               tenant_id = EXCLUDED.tenant_id",
        )
        .bind(project_id)
        .bind(agent_id)
        .bind(is_default)
        .bind(&added_at)
        .bind(&tenant_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(ProjectAgent {
            project_id: project_id.to_string(),
            agent_id: agent_id.to_string(),
            is_default,
            added_at,
        })
    }

    async fn remove_agent(&self, project_id: &str, agent_id: &str) -> Result<(), String> {
        let tenant_id = self.tenant_id();
        sqlx::query(
            "DELETE FROM project_agents WHERE project_id = $1 AND agent_id = $2 AND tenant_id = $3",
        )
        .bind(project_id)
        .bind(agent_id)
        .bind(&tenant_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        sqlx::query(
            "UPDATE work_items SET assignee_agent_id = NULL, updated_at = $1
             WHERE project_id = $2 AND assignee_agent_id = $3 AND tenant_id = $4",
        )
        .bind(now())
        .bind(project_id)
        .bind(agent_id)
        .bind(&tenant_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }
}

#[async_trait]
impl ScheduleRepo for PgRepos {
    async fn list(&self) -> Result<Vec<Schedule>, String> {
        let rows = sqlx::query(&format!(
            "SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE tenant_id = $1 ORDER BY created_at DESC"
        ))
        .bind(self.tenant_id())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.iter()
            .map(map_schedule_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn list_for_task(&self, task_id: &str) -> Result<Vec<Schedule>, String> {
        let rows = sqlx::query(&format!(
            "SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE task_id = $1 AND tenant_id = $2"
        ))
        .bind(task_id)
        .bind(self.tenant_id())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.iter()
            .map(map_schedule_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn list_for_workflow(&self, workflow_id: &str) -> Result<Vec<Schedule>, String> {
        let rows = sqlx::query(&format!(
            "SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE workflow_id = $1 AND tenant_id = $2"
        ))
        .bind(workflow_id)
        .bind(self.tenant_id())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.iter()
            .map(map_schedule_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn create(&self, payload: CreateSchedule) -> Result<Schedule, String> {
        let tenant_id = self.tenant_id();
        let id = Ulid::new().to_string();
        let now = now();
        let target_kind = payload
            .target_kind
            .clone()
            .unwrap_or_else(|| "task".to_string());
        match target_kind.as_str() {
            "task" if payload.task_id.is_some() && payload.workflow_id.is_none() => {}
            "workflow" if payload.workflow_id.is_some() && payload.task_id.is_none() => {}
            "task" => return Err("task schedule requires task_id and no workflow_id".into()),
            "workflow" => {
                return Err("workflow schedule requires workflow_id and no task_id".into())
            }
            other => return Err(format!("invalid target_kind: {other}")),
        }
        let next_run_at = if payload.kind == "recurring" {
            serde_json::from_value::<RecurringConfig>(payload.config.clone())
                .ok()
                .and_then(|cfg| next_n_runs(&cfg, 1).into_iter().next())
        } else {
            None
        };
        sqlx::query(
            "INSERT INTO schedules (id, task_id, workflow_id, target_kind, kind, config, enabled,
             next_run_at, created_at, updated_at, tenant_id)
             VALUES ($1,$2,$3,$4,$5,$6,true,$7,$8,$8,$9)",
        )
        .bind(&id)
        .bind(payload.task_id)
        .bind(payload.workflow_id)
        .bind(target_kind)
        .bind(payload.kind)
        .bind(json_string(&payload.config)?)
        .bind(next_run_at)
        .bind(now)
        .bind(&tenant_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        fetch_schedule(&self.pool, &id, &tenant_id).await
    }

    async fn toggle(&self, id: &str, enabled: bool) -> Result<Schedule, String> {
        let tenant_id = self.tenant_id();
        sqlx::query(
            "UPDATE schedules SET enabled = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4",
        )
        .bind(enabled)
        .bind(now())
        .bind(id)
        .bind(&tenant_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        fetch_schedule(&self.pool, id, &tenant_id).await
    }

    async fn delete(&self, id: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM schedules WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(self.tenant_id())
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

#[async_trait]
impl UserRepo for PgRepos {
    async fn list(&self) -> Result<Vec<User>, String> {
        let rows = sqlx::query(
            "SELECT id, name, is_default, created_at::text FROM users WHERE tenant_id = $1 ORDER BY created_at ASC",
        )
        .bind(self.tenant_id()).fetch_all(&self.pool).await.map_err(db_err)?;
        rows.iter()
            .map(map_user_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn create(&self, name: String) -> Result<User, String> {
        let id = Ulid::new().to_string();
        let created_at = now();
        sqlx::query("INSERT INTO users (id, name, is_default, created_at, tenant_id) VALUES ($1,$2,false,$3,$4)")
            .bind(&id).bind(&name).bind(&created_at).bind(self.tenant_id()).execute(&self.pool).await.map_err(db_err)?;
        Ok(User {
            id,
            name,
            is_default: false,
            created_at,
        })
    }

    async fn exists(&self, id: &str) -> Result<bool, String> {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1 AND tenant_id = $2)")
            .bind(id)
            .bind(self.tenant_id())
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)
    }
}

const RUN_SUMMARY_SELECT: &str = "SELECT r.id, r.task_id, COALESCE(t.name, '') AS task_name,
    r.schedule_id, r.agent_id, a.name AS agent_name, r.state, r.trigger, r.exit_code,
    r.started_at::text, r.finished_at::text, r.duration_ms, r.retry_count, r.is_sub_agent,
    r.created_at::text, r.metadata::jsonb->>'chat_session_id' AS chat_session_id, r.project_id
    FROM runs r
    LEFT JOIN tasks t ON t.id = r.task_id AND t.tenant_id = r.tenant_id
    LEFT JOIN agents a ON a.id = r.agent_id AND a.tenant_id = r.tenant_id";

fn map_run_summary_row(row: &PgRow) -> Result<RunSummary, sqlx::Error> {
    Ok(RunSummary {
        id: row.try_get(0)?,
        task_id: row.try_get(1)?,
        task_name: row.try_get(2)?,
        schedule_id: row.try_get(3)?,
        agent_id: row.try_get(4)?,
        agent_name: row.try_get(5)?,
        state: row.try_get(6)?,
        trigger: row.try_get(7)?,
        exit_code: row.try_get(8)?,
        started_at: row.try_get(9)?,
        finished_at: row.try_get(10)?,
        duration_ms: row.try_get(11)?,
        retry_count: row.try_get(12)?,
        is_sub_agent: bool_row(row, 13)?,
        created_at: row.try_get(14)?,
        chat_session_id: row.try_get(15)?,
        project_id: row.try_get(16)?,
    })
}

#[async_trait]
impl RunRepo for PgRepos {
    async fn list(&self, filter: RunListFilter) -> Result<Vec<RunSummary>, String> {
        let tenant_id = self.tenant_id();
        let mut qb = QueryBuilder::new(format!("{RUN_SUMMARY_SELECT} WHERE r.tenant_id = "));
        qb.push_bind(tenant_id);
        if let Some(task_id) = filter.task_id {
            qb.push(" AND r.task_id = ").push_bind(task_id);
        }
        if let Some(state) = filter.state_filter {
            if state != "all" {
                qb.push(" AND r.state = ").push_bind(state);
            }
        }
        if let Some(project_id) = filter.project_id {
            qb.push(" AND r.project_id = ").push_bind(project_id);
        }
        qb.push(" ORDER BY r.created_at DESC LIMIT ")
            .push_bind(filter.limit.unwrap_or(100))
            .push(" OFFSET ")
            .push_bind(filter.offset.unwrap_or(0));
        let rows = qb.build().fetch_all(&self.pool).await.map_err(db_err)?;
        rows.iter()
            .map(map_run_summary_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn get(&self, id: &str) -> Result<Option<Run>, String> {
        let row = sqlx::query(
            "SELECT id, task_id, schedule_id, agent_id, state, trigger, exit_code, pid,
                    log_path, started_at::text, finished_at::text, duration_ms, retry_count,
                    parent_run_id, metadata::text, is_sub_agent, created_at::text, project_id
             FROM runs WHERE id = $1 AND tenant_id = $2",
        )
        .bind(id)
        .bind(self.tenant_id())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        row.map(|row| {
            let meta: String = row.try_get(14).map_err(db_err)?;
            Ok(Run {
                id: row.try_get(0).map_err(db_err)?,
                task_id: row.try_get(1).map_err(db_err)?,
                schedule_id: row.try_get(2).map_err(db_err)?,
                agent_id: row.try_get(3).map_err(db_err)?,
                state: row.try_get(4).map_err(db_err)?,
                trigger: row.try_get(5).map_err(db_err)?,
                exit_code: row.try_get(6).map_err(db_err)?,
                pid: row.try_get(7).map_err(db_err)?,
                log_path: row.try_get(8).map_err(db_err)?,
                started_at: row.try_get(9).map_err(db_err)?,
                finished_at: row.try_get(10).map_err(db_err)?,
                duration_ms: row.try_get(11).map_err(db_err)?,
                retry_count: row.try_get(12).map_err(db_err)?,
                parent_run_id: row.try_get(13).map_err(db_err)?,
                metadata: parse_json(&meta),
                is_sub_agent: row.try_get(15).map_err(db_err)?,
                created_at: row.try_get(16).map_err(db_err)?,
                project_id: row.try_get(17).map_err(db_err)?,
            })
        })
        .transpose()
    }

    async fn list_active(&self) -> Result<Vec<RunSummary>, String> {
        let rows = sqlx::query(&format!(
            "{RUN_SUMMARY_SELECT} WHERE r.tenant_id = $1 AND r.state IN ('pending','queued','running') ORDER BY r.created_at DESC"
        ))
        .bind(self.tenant_id()).fetch_all(&self.pool).await.map_err(db_err)?;
        rows.iter()
            .map(map_run_summary_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn list_sub_agents(&self, parent_run_id: &str) -> Result<Vec<RunSummary>, String> {
        let rows = sqlx::query(&format!(
            "{RUN_SUMMARY_SELECT} WHERE r.parent_run_id = $1 AND r.tenant_id = $2 AND r.is_sub_agent = true ORDER BY r.created_at ASC"
        ))
        .bind(parent_run_id).bind(self.tenant_id()).fetch_all(&self.pool).await.map_err(db_err)?;
        rows.iter()
            .map(map_run_summary_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn agent_conversation(&self, run_id: &str) -> Result<Option<serde_json::Value>, String> {
        let raw: Option<String> = sqlx::query_scalar(
            "SELECT messages::text FROM agent_conversations WHERE run_id = $1 AND tenant_id = $2",
        )
        .bind(run_id)
        .bind(self.tenant_id())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(raw.map(|raw| parse_json(&raw)))
    }

    async fn log_path(&self, run_id: &str) -> Result<Option<String>, String> {
        sqlx::query_scalar("SELECT log_path FROM runs WHERE id = $1 AND tenant_id = $2")
            .bind(run_id)
            .bind(self.tenant_id())
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)
    }

    async fn cancel(&self, run_id: &str) -> Result<(), String> {
        sqlx::query(
            "UPDATE runs SET state = 'cancelled', finished_at = $1
             WHERE id = $2 AND tenant_id = $3 AND state IN ('pending','queued','running')",
        )
        .bind(now())
        .bind(run_id)
        .bind(self.tenant_id())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }
}

#[async_trait]
impl WorkItemEventRepo for PgRepos {
    async fn list(&self, work_item_id: &str) -> Result<Vec<WorkItemEvent>, String> {
        let rows = sqlx::query(&format!(
            "SELECT {WORK_ITEM_EVENT_COLUMNS} FROM work_item_events
             WHERE work_item_id = $1 AND tenant_id = $2
             ORDER BY created_at ASC, id ASC"
        ))
        .bind(work_item_id)
        .bind(self.tenant_id())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.iter()
            .map(map_work_item_event_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }
}

#[async_trait]
impl ProjectBoardRepo for PgRepos {
    async fn list(&self, project_id: &str) -> Result<Vec<ProjectBoard>, String> {
        let rows = sqlx::query(&format!(
            "SELECT {PROJECT_BOARD_COLUMNS} FROM project_boards
             WHERE project_id = $1 AND tenant_id = $2
             ORDER BY is_default DESC, position ASC, created_at ASC"
        ))
        .bind(project_id)
        .bind(self.tenant_id())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.iter()
            .map(map_project_board_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn get(&self, id: &str) -> Result<Option<ProjectBoard>, String> {
        let row = sqlx::query(&format!(
            "SELECT {PROJECT_BOARD_COLUMNS} FROM project_boards WHERE id = $1 AND tenant_id = $2"
        ))
        .bind(id)
        .bind(self.tenant_id())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        row.map(|row| map_project_board_row(&row).map_err(db_err))
            .transpose()
    }

    async fn create(&self, payload: CreateProjectBoard) -> Result<ProjectBoard, String> {
        validate_board_prefix(&payload.prefix)?;
        let name = payload.name.trim().to_string();
        if name.is_empty() {
            return Err("board name must be non-empty".into());
        }
        let tenant_id = self.tenant_id();
        let prefix = payload.prefix.trim().to_string();
        let taken: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM project_boards WHERE project_id = $1 AND prefix = $2 AND tenant_id = $3)",
        )
        .bind(&payload.project_id).bind(&prefix).bind(&tenant_id).fetch_one(&self.pool).await.map_err(db_err)?;
        if taken {
            return Err(format!(
                "a board with prefix '{prefix}' already exists in this project"
            ));
        }
        let position: f64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(position), 0) FROM project_boards WHERE project_id = $1 AND tenant_id = $2",
        )
        .bind(&payload.project_id).bind(&tenant_id).fetch_one(&self.pool).await.map_err(db_err)?;
        let id = Ulid::new().to_string();
        let now = now();
        sqlx::query(
            "INSERT INTO project_boards (id, project_id, name, prefix, position, is_default, created_at, updated_at, tenant_id)
             VALUES ($1,$2,$3,$4,$5,false,$6,$6,$7)",
        )
        .bind(&id).bind(payload.project_id).bind(name).bind(prefix).bind(position + 1024.0).bind(now).bind(&tenant_id)
        .execute(&self.pool).await.map_err(db_err)?;
        ProjectBoardRepo::get(self, &id)
            .await?
            .ok_or_else(|| format!("board not found after insert: {id}"))
    }

    async fn update(&self, id: &str, payload: UpdateProjectBoard) -> Result<ProjectBoard, String> {
        let tenant_id = self.tenant_id();
        let existing = ProjectBoardRepo::get(self, id)
            .await?
            .ok_or_else(|| format!("board '{id}' not found"))?;
        let now = now();
        if let Some(name) = payload.name {
            let name = name.trim().to_string();
            if name.is_empty() {
                return Err("board name must be non-empty".into());
            }
            sqlx::query("UPDATE project_boards SET name = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(name).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        if let Some(prefix) = payload.prefix {
            validate_board_prefix(&prefix)?;
            let prefix = prefix.trim().to_string();
            let taken: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM project_boards WHERE project_id = $1 AND prefix = $2 AND id <> $3 AND tenant_id = $4)",
            )
            .bind(&existing.project_id).bind(&prefix).bind(id).bind(&tenant_id).fetch_one(&self.pool).await.map_err(db_err)?;
            if taken {
                return Err(format!(
                    "a board with prefix '{prefix}' already exists in this project"
                ));
            }
            sqlx::query("UPDATE project_boards SET prefix = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(prefix).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        ProjectBoardRepo::get(self, id)
            .await?
            .ok_or_else(|| format!("board '{id}' not found"))
    }

    async fn delete(&self, id: &str, payload: DeleteProjectBoard) -> Result<(), String> {
        let tenant_id = self.tenant_id();
        let existing = ProjectBoardRepo::get(self, id)
            .await?
            .ok_or_else(|| format!("board '{id}' not found"))?;
        let siblings = ProjectBoardRepo::list(self, &existing.project_id).await?;
        if siblings.len() <= 1 {
            return Err("cannot delete the last remaining board".into());
        }
        let item_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM work_items WHERE board_id = $1 AND tenant_id = $2",
        )
        .bind(id)
        .bind(&tenant_id)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        if item_count > 0
            && payload.destination_board_id.is_none()
            && !payload.force.unwrap_or(false)
        {
            return Err("choose a destination board before deleting a board that has items".into());
        }
        let now = now();
        if let Some(dest_id) = payload.destination_board_id {
            let dest = ProjectBoardRepo::get(self, &dest_id)
                .await?
                .ok_or_else(|| format!("destination board '{dest_id}' not found"))?;
            if dest.project_id != existing.project_id || dest.id == existing.id {
                return Err("destination board must be another board in the same project".into());
            }
            sqlx::query("UPDATE project_board_columns SET board_id = $1, updated_at = $2 WHERE board_id = $3 AND tenant_id = $4")
                .bind(&dest.id).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
            sqlx::query("UPDATE work_items SET board_id = $1, updated_at = $2 WHERE board_id = $3 AND tenant_id = $4")
                .bind(&dest.id).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        if existing.is_default {
            if let Some(next_default) = siblings.iter().find(|board| board.id != id) {
                sqlx::query("UPDATE project_boards SET is_default = false, updated_at = $1 WHERE id = $2 AND tenant_id = $3")
                    .bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
                sqlx::query("UPDATE project_boards SET is_default = true, updated_at = $1 WHERE id = $2 AND tenant_id = $3")
                    .bind(&now).bind(&next_default.id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
            }
        }
        sqlx::query("DELETE FROM project_boards WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(&tenant_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

#[async_trait]
impl ProjectBoardColumnRepo for PgRepos {
    async fn list(
        &self,
        project_id: &str,
        board_id: Option<String>,
    ) -> Result<Vec<ProjectBoardColumn>, String> {
        let tenant_id = self.tenant_id();
        let effective_board_id = match board_id {
            Some(id) => Some(id),
            None => sqlx::query_scalar(
                "SELECT id FROM project_boards WHERE project_id = $1 AND tenant_id = $2 AND is_default = true LIMIT 1",
            )
            .bind(project_id).bind(&tenant_id).fetch_optional(&self.pool).await.map_err(db_err)?,
        };
        let rows = if let Some(board_id) = effective_board_id {
            sqlx::query(&format!(
                "SELECT {PROJECT_BOARD_COLUMN_COLUMNS} FROM project_board_columns
                 WHERE project_id = $1 AND board_id = $2 AND tenant_id = $3
                 ORDER BY position ASC, created_at ASC"
            ))
            .bind(project_id)
            .bind(board_id)
            .bind(&tenant_id)
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?
        } else {
            sqlx::query(&format!(
                "SELECT {PROJECT_BOARD_COLUMN_COLUMNS} FROM project_board_columns
                 WHERE project_id = $1 AND tenant_id = $2
                 ORDER BY position ASC, created_at ASC"
            ))
            .bind(project_id)
            .bind(&tenant_id)
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?
        };
        rows.iter()
            .map(map_project_board_column_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn get(&self, id: &str) -> Result<Option<ProjectBoardColumn>, String> {
        let row = sqlx::query(&format!(
            "SELECT {PROJECT_BOARD_COLUMN_COLUMNS} FROM project_board_columns WHERE id = $1 AND tenant_id = $2"
        ))
        .bind(id).bind(self.tenant_id()).fetch_optional(&self.pool).await.map_err(db_err)?;
        row.map(|row| map_project_board_column_row(&row).map_err(db_err))
            .transpose()
    }
}

#[async_trait]
impl BusMessageRepo for PgRepos {
    async fn list(
        &self,
        agent_id: Option<String>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<BusMessage>, String> {
        let tenant_id = self.tenant_id();
        let rows = if let Some(agent_id) = agent_id {
            sqlx::query(&format!(
                "SELECT {BUS_MESSAGE_COLUMNS} FROM bus_messages
                 WHERE tenant_id = $1 AND (from_agent_id = $2 OR to_agent_id = $2)
                 ORDER BY created_at DESC LIMIT $3 OFFSET $4"
            ))
            .bind(&tenant_id)
            .bind(agent_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?
        } else {
            sqlx::query(&format!(
                "SELECT {BUS_MESSAGE_COLUMNS} FROM bus_messages
                 WHERE tenant_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3"
            ))
            .bind(&tenant_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?
        };
        rows.iter()
            .map(map_bus_message_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn thread_for_agent(
        &self,
        agent_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<PaginatedBusThread, String> {
        let tenant_id = self.tenant_id();
        let total_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM bus_messages WHERE to_agent_id = $1 AND tenant_id = $2",
        )
        .bind(agent_id)
        .bind(&tenant_id)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        let rows = sqlx::query(
            "SELECT bm.id, bm.from_agent_id, COALESCE(a.name, bm.from_agent_id), bm.to_agent_id, bm.kind,
                    bm.payload::text, bm.status, bm.created_at::text,
                    bm.to_run_id, r.state, r.metadata::jsonb->>'finish_summary',
                    bm.to_session_id, cs.execution_state, cs.finish_summary
             FROM bus_messages bm
             LEFT JOIN agents a ON a.id = bm.from_agent_id AND a.tenant_id = bm.tenant_id
             LEFT JOIN runs r ON r.id = bm.to_run_id AND r.tenant_id = bm.tenant_id
             LEFT JOIN chat_sessions cs ON cs.id = bm.to_session_id AND cs.tenant_id = bm.tenant_id
             WHERE bm.to_agent_id = $1 AND bm.tenant_id = $2
             ORDER BY bm.created_at DESC LIMIT $3 OFFSET $4",
        )
        .bind(agent_id).bind(&tenant_id).bind(limit).bind(offset).fetch_all(&self.pool).await.map_err(db_err)?;
        let messages = rows
            .iter()
            .map(|row| {
                let payload: String = row.try_get(5)?;
                Ok(BusThreadMessage {
                    id: row.try_get(0)?,
                    from_agent_id: row.try_get(1)?,
                    from_agent_name: row.try_get(2)?,
                    to_agent_id: row.try_get(3)?,
                    kind: row.try_get(4)?,
                    payload: parse_json(&payload),
                    status: row.try_get(6)?,
                    created_at: row.try_get(7)?,
                    triggered_run_id: row.try_get(8)?,
                    triggered_run_state: row.try_get(9)?,
                    triggered_run_summary: row.try_get(10)?,
                    triggered_session_id: row.try_get(11)?,
                    triggered_session_state: row.try_get(12)?,
                    triggered_session_summary: row.try_get(13)?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(db_err)?;
        Ok(PaginatedBusThread {
            messages,
            total_count,
            has_more: offset + limit < total_count,
        })
    }
}

#[async_trait]
impl BusSubscriptionRepo for PgRepos {
    async fn list(&self, agent_id: Option<String>) -> Result<Vec<BusSubscription>, String> {
        let tenant_id = self.tenant_id();
        let rows = if let Some(agent_id) = agent_id {
            sqlx::query(&format!(
                "SELECT {BUS_SUBSCRIPTION_COLUMNS} FROM bus_subscriptions
                 WHERE tenant_id = $1 AND (subscriber_agent_id = $2 OR source_agent_id = $2)
                 ORDER BY created_at DESC"
            ))
            .bind(&tenant_id)
            .bind(agent_id)
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?
        } else {
            sqlx::query(&format!(
                "SELECT {BUS_SUBSCRIPTION_COLUMNS} FROM bus_subscriptions
                 WHERE tenant_id = $1 ORDER BY created_at DESC"
            ))
            .bind(&tenant_id)
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?
        };
        rows.iter()
            .map(map_bus_subscription_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn create(&self, payload: CreateBusSubscription) -> Result<BusSubscription, String> {
        let id = Ulid::new().to_string();
        let now = now();
        let tenant_id = self.tenant_id();
        sqlx::query(
            "INSERT INTO bus_subscriptions (id, subscriber_agent_id, source_agent_id, event_type,
             task_id, payload_template, enabled, max_chain_depth, created_at, updated_at, tenant_id)
             VALUES ($1,$2,$3,$4,$5,$6,true,$7,$8,$8,$9)",
        )
        .bind(&id)
        .bind(&payload.subscriber_agent_id)
        .bind(&payload.source_agent_id)
        .bind(&payload.event_type)
        .bind(&payload.task_id)
        .bind(&payload.payload_template)
        .bind(payload.max_chain_depth)
        .bind(&now)
        .bind(&tenant_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
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
    }

    async fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        sqlx::query("UPDATE bus_subscriptions SET enabled = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
            .bind(enabled).bind(now()).bind(id).bind(self.tenant_id()).execute(&self.pool).await.map_err(db_err)?;
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM bus_subscriptions WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(self.tenant_id())
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

#[async_trait]
impl ChatRepo for PgRepos {
    async fn list_sessions(
        &self,
        filter: ChatSessionListFilter,
    ) -> Result<Vec<ChatSession>, String> {
        let tenant_id = self.tenant_id();
        let mut qb = QueryBuilder::new(format!("{CHAT_SESSION_SELECT} WHERE cs.agent_id = "));
        qb.push_bind(filter.agent_id)
            .push(" AND cs.tenant_id = ")
            .push_bind(tenant_id);
        if !filter.include_archived {
            qb.push(" AND cs.archived = false");
        }
        if let Some(project_id) = filter.project_id {
            qb.push(" AND cs.project_id = ").push_bind(project_id);
        }
        if !filter.session_types.is_empty() {
            qb.push(" AND cs.session_type IN (");
            let mut separated = qb.separated(", ");
            for value in filter.session_types {
                separated.push_bind(value);
            }
            separated.push_unseparated(")");
        }
        qb.push(" ORDER BY cs.updated_at DESC");
        let rows = qb.build().fetch_all(&self.pool).await.map_err(db_err)?;
        rows.iter()
            .map(map_chat_session_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn get_messages(
        &self,
        session_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<ChatMessageRows, String> {
        let tenant_id = self.tenant_id();
        let total_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM chat_messages WHERE session_id = $1 AND tenant_id = $2",
        )
        .bind(session_id)
        .bind(&tenant_id)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        let rows =
            if limit > 0 {
                sqlx::query(
                    "SELECT id, role, content::text, created_at::text, is_compacted FROM (
                   SELECT id, role, content, created_at, is_compacted
                   FROM chat_messages WHERE session_id = $1 AND tenant_id = $2
                   ORDER BY created_at DESC LIMIT $3 OFFSET $4
                 ) sub ORDER BY created_at ASC",
                )
                .bind(session_id)
                .bind(&tenant_id)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            } else {
                sqlx::query(
                "SELECT id, role, content::text, created_at::text, is_compacted FROM chat_messages
                 WHERE session_id = $1 AND tenant_id = $2 ORDER BY created_at ASC",
            )
            .bind(session_id).bind(&tenant_id).fetch_all(&self.pool).await.map_err(db_err)?
            };
        let messages = rows
            .iter()
            .map(map_chat_message_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(ChatMessageRows {
            messages,
            total_count,
            has_more: limit > 0 && offset + limit < total_count,
        })
    }

    async fn create_session(
        &self,
        agent_id: String,
        title: Option<String>,
        session_type: Option<String>,
        project_id: Option<String>,
    ) -> Result<ChatSession, String> {
        let id = Ulid::new().to_string();
        let now = now();
        let title = title.unwrap_or_else(|| "New Chat".to_string());
        let session_type = session_type.unwrap_or_else(|| "user_chat".to_string());
        sqlx::query(
            "INSERT INTO chat_sessions (
                id, agent_id, title, archived, session_type, parent_session_id, source_bus_message_id,
                chain_depth, execution_state, finish_summary, terminal_error, project_id, created_at, updated_at, tenant_id
             ) VALUES ($1,$2,$3,false,$4,NULL,NULL,0,NULL,NULL,NULL,$5,$6,$6,$7)",
        )
        .bind(&id).bind(&agent_id).bind(&title).bind(&session_type).bind(&project_id).bind(&now).bind(self.tenant_id())
        .execute(&self.pool).await.map_err(db_err)?;
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
    }

    async fn rename_session(&self, session_id: &str, title: String) -> Result<String, String> {
        let updated_at = now();
        sqlx::query(
            "UPDATE chat_sessions SET title = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4",
        )
        .bind(title)
        .bind(&updated_at)
        .bind(session_id)
        .bind(self.tenant_id())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(updated_at)
    }

    async fn archive_session(&self, session_id: &str) -> Result<String, String> {
        self.set_archive_state(session_id, true).await
    }

    async fn unarchive_session(&self, session_id: &str) -> Result<String, String> {
        self.set_archive_state(session_id, false).await
    }

    async fn delete_session(&self, session_id: &str) -> Result<(), String> {
        let tenant_id = self.tenant_id();
        let state: Option<String> = sqlx::query_scalar(
            "SELECT execution_state FROM chat_sessions WHERE id = $1 AND tenant_id = $2",
        )
        .bind(session_id)
        .bind(&tenant_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        if matches!(state.as_deref(), Some("queued") | Some("running")) {
            return Err("cannot delete an active agent session".to_string());
        }
        sqlx::query("DELETE FROM chat_sessions WHERE id = $1 AND tenant_id = $2")
            .bind(session_id)
            .bind(&tenant_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn append_message(
        &self,
        session_id: &str,
        role: &str,
        content_json: String,
    ) -> Result<(String, String), String> {
        let id = Ulid::new().to_string();
        let created_at = now();
        let tenant_id = self.tenant_id();
        sqlx::query(
            "INSERT INTO chat_messages (id, session_id, role, content, created_at, tenant_id)
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(&id)
        .bind(session_id)
        .bind(role)
        .bind(content_json)
        .bind(&created_at)
        .bind(&tenant_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        sqlx::query("UPDATE chat_sessions SET updated_at = $1 WHERE id = $2 AND tenant_id = $3")
            .bind(&created_at)
            .bind(session_id)
            .bind(&tenant_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok((id, created_at))
    }

    async fn upsert_active_skill(
        &self,
        session_id: &str,
        skill_name: &str,
        instructions: &str,
        source_path: Option<String>,
    ) -> Result<(), String> {
        sqlx::query(
            "INSERT INTO active_session_skills (session_id, skill_name, instructions, source_path, activated_at, tenant_id)
             VALUES ($1,$2,$3,$4,$5,$6)
             ON CONFLICT(session_id, skill_name) DO UPDATE SET
               instructions = EXCLUDED.instructions,
               source_path = EXCLUDED.source_path,
               activated_at = EXCLUDED.activated_at,
               tenant_id = EXCLUDED.tenant_id",
        )
        .bind(session_id).bind(skill_name).bind(instructions).bind(source_path).bind(now()).bind(self.tenant_id())
        .execute(&self.pool).await.map_err(db_err)?;
        Ok(())
    }

    async fn session_meta(&self, session_id: &str) -> Result<ChatSessionMeta, String> {
        let row = sqlx::query(
            "SELECT cs.agent_id, cs.project_id, p.name
             FROM chat_sessions cs
             LEFT JOIN projects p ON p.id = cs.project_id AND p.tenant_id = cs.tenant_id
             WHERE cs.id = $1 AND cs.tenant_id = $2",
        )
        .bind(session_id)
        .bind(self.tenant_id())
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(ChatSessionMeta {
            session_id: session_id.to_string(),
            agent_id: row.try_get(0).map_err(db_err)?,
            project_id: row.try_get(1).map_err(db_err)?,
            project_name: row.try_get(2).map_err(db_err)?,
        })
    }

    async fn session_execution(&self, session_id: &str) -> Result<SessionExecutionStatus, String> {
        let row = sqlx::query(
            "SELECT execution_state, finish_summary, terminal_error
             FROM chat_sessions WHERE id = $1 AND tenant_id = $2",
        )
        .bind(session_id)
        .bind(self.tenant_id())
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(SessionExecutionStatus {
            session_id: session_id.to_string(),
            execution_state: row.try_get(0).map_err(db_err)?,
            finish_summary: row.try_get(1).map_err(db_err)?,
            terminal_error: row.try_get(2).map_err(db_err)?,
        })
    }

    async fn session_type(&self, session_id: &str) -> Result<String, String> {
        sqlx::query_scalar(
            "SELECT session_type FROM chat_sessions WHERE id = $1 AND tenant_id = $2",
        )
        .bind(session_id)
        .bind(self.tenant_id())
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)
    }

    async fn list_message_reactions(
        &self,
        session_id: &str,
    ) -> Result<Vec<MessageReactionRow>, String> {
        let rows = sqlx::query(
            "SELECT id, message_id, emoji, created_at::text FROM message_reactions
             WHERE session_id = $1 AND tenant_id = $2 ORDER BY created_at ASC",
        )
        .bind(session_id)
        .bind(self.tenant_id())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.iter()
            .map(|row| {
                Ok(MessageReactionRow {
                    id: row.try_get(0)?,
                    message_id: row.try_get(1)?,
                    emoji: row.try_get(2)?,
                    created_at: row.try_get(3)?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(db_err)
    }

    async fn token_usage(&self, session_id: &str) -> Result<ChatSessionTokenUsage, String> {
        let row = sqlx::query("SELECT last_input_tokens, agent_id FROM chat_sessions WHERE id = $1 AND tenant_id = $2")
            .bind(session_id).bind(self.tenant_id()).fetch_one(&self.pool).await.map_err(db_err)?;
        let tokens: Option<i64> = row.try_get(0).map_err(db_err)?;
        Ok(ChatSessionTokenUsage {
            last_input_tokens: tokens.map(|value| value as u32),
            agent_id: row.try_get(1).map_err(db_err)?,
        })
    }
}

impl PgRepos {
    async fn set_archive_state(&self, session_id: &str, archived: bool) -> Result<String, String> {
        let tenant_id = self.tenant_id();
        let state: Option<String> = sqlx::query_scalar(
            "SELECT execution_state FROM chat_sessions WHERE id = $1 AND tenant_id = $2",
        )
        .bind(session_id)
        .bind(&tenant_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        if archived && matches!(state.as_deref(), Some("queued") | Some("running")) {
            return Err("cannot archive an active agent session".to_string());
        }
        let updated_at = now();
        sqlx::query("UPDATE chat_sessions SET archived = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
            .bind(archived).bind(&updated_at).bind(session_id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        sqlx::query("UPDATE chat_sessions SET archived = $1, updated_at = $2 WHERE parent_session_id = $3 AND tenant_id = $4")
            .bind(archived).bind(&updated_at).bind(session_id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        sqlx::query(
            "UPDATE chat_sessions SET archived = $1, updated_at = $2
             WHERE tenant_id = $4
               AND id IN (
                 SELECT bm.to_session_id FROM bus_messages bm
                 WHERE bm.from_session_id = $3 AND bm.tenant_id = $4 AND bm.to_session_id IS NOT NULL
               )",
        )
        .bind(archived).bind(&updated_at).bind(session_id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        Ok(updated_at)
    }
}

#[async_trait]
impl WorkItemRepo for PgRepos {
    async fn list(
        &self,
        project_id: &str,
        board_id: Option<String>,
    ) -> Result<Vec<WorkItem>, String> {
        let tenant_id = self.tenant_id();
        let rows = if let Some(board_id) = board_id {
            sqlx::query(&format!(
                "SELECT {WORK_ITEM_COLUMNS} FROM work_items
                 WHERE project_id = $1 AND board_id = $2 AND tenant_id = $3
                 ORDER BY position ASC, created_at ASC"
            ))
            .bind(project_id)
            .bind(board_id)
            .bind(&tenant_id)
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?
        } else {
            sqlx::query(&format!(
                "SELECT {WORK_ITEM_COLUMNS} FROM work_items
                 WHERE project_id = $1 AND tenant_id = $2
                 ORDER BY position ASC, created_at ASC"
            ))
            .bind(project_id)
            .bind(&tenant_id)
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?
        };
        rows.iter()
            .map(map_work_item_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn get(&self, id: &str) -> Result<WorkItem, String> {
        fetch_work_item(&self.pool, id, &self.tenant_id()).await
    }

    async fn lookup_project_id(&self, id: &str) -> Result<String, String> {
        sqlx::query_scalar("SELECT project_id FROM work_items WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(self.tenant_id())
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)
    }

    async fn create(&self, payload: CreateWorkItem) -> Result<WorkItem, String> {
        let tenant_id = self.tenant_id();
        let id = Ulid::new().to_string();
        let now = now();
        let labels = serde_json::to_string(&payload.labels.unwrap_or_default()).map_err(db_err)?;
        let metadata = json_string(&payload.metadata.unwrap_or_else(|| serde_json::json!({})))?;
        let status = payload.status.unwrap_or_else(|| "todo".to_string());
        let kind = payload.kind.unwrap_or_else(|| "task".to_string());
        sqlx::query(
            "INSERT INTO work_items (
                id, project_id, board_id, title, description, kind, column_id, status, priority,
                assignee_agent_id, created_by_agent_id, parent_work_item_id, position, labels,
                metadata, created_at, updated_at, tenant_id
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$16,$17)",
        )
        .bind(&id)
        .bind(&payload.project_id)
        .bind(payload.board_id)
        .bind(payload.title)
        .bind(payload.description)
        .bind(kind)
        .bind(payload.column_id)
        .bind(status)
        .bind(payload.priority.unwrap_or(1))
        .bind(payload.assignee_agent_id)
        .bind(payload.created_by_agent_id.as_deref())
        .bind(payload.parent_work_item_id)
        .bind(payload.position.unwrap_or(0.0))
        .bind(labels)
        .bind(metadata)
        .bind(&now)
        .bind(&tenant_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        insert_work_item_event(
            &self.pool,
            &tenant_id,
            &id,
            if payload.created_by_agent_id.is_some() {
                "agent"
            } else {
                "system"
            },
            payload.created_by_agent_id.as_deref(),
            "created",
            serde_json::json!({}),
        )
        .await?;
        fetch_work_item(&self.pool, &id, &tenant_id).await
    }

    async fn update(&self, id: &str, payload: UpdateWorkItem) -> Result<WorkItem, String> {
        let tenant_id = self.tenant_id();
        let now = now();
        if let Some(v) = payload.title {
            sqlx::query("UPDATE work_items SET title = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(v).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        if let Some(v) = payload.description {
            sqlx::query("UPDATE work_items SET description = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(v).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        if let Some(v) = payload.kind {
            sqlx::query(
                "UPDATE work_items SET kind = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4",
            )
            .bind(v)
            .bind(&now)
            .bind(id)
            .bind(&tenant_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        }
        if let Some(v) = payload.column_id {
            let column_id = if v.is_empty() { None } else { Some(v) };
            sqlx::query("UPDATE work_items SET column_id = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(column_id).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        if let Some(v) = payload.priority {
            sqlx::query("UPDATE work_items SET priority = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(v).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        if let Some(v) = payload.labels {
            sqlx::query("UPDATE work_items SET labels = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(serde_json::to_string(&v).map_err(db_err)?).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        if let Some(v) = payload.metadata {
            sqlx::query("UPDATE work_items SET metadata = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(json_string(&v)?).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        insert_work_item_event(
            &self.pool,
            &tenant_id,
            id,
            "system",
            None,
            "updated",
            serde_json::json!({}),
        )
        .await?;
        fetch_work_item(&self.pool, id, &tenant_id).await
    }

    async fn delete(&self, id: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM work_items WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(self.tenant_id())
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn claim(&self, id: &str, agent_id: &str) -> Result<WorkItem, String> {
        let tenant_id = self.tenant_id();
        sqlx::query("UPDATE work_items SET assignee_agent_id = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
            .bind(agent_id).bind(now()).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        insert_work_item_event(
            &self.pool,
            &tenant_id,
            id,
            "agent",
            Some(agent_id),
            "claimed",
            serde_json::json!({ "agentId": agent_id }),
        )
        .await?;
        fetch_work_item(&self.pool, id, &tenant_id).await
    }

    async fn move_item(
        &self,
        id: &str,
        column_id: Option<String>,
        position: Option<f64>,
    ) -> Result<WorkItem, String> {
        let tenant_id = self.tenant_id();
        let status: Option<String> = if let Some(column_id) = column_id.as_deref() {
            sqlx::query_scalar("SELECT COALESCE(role, name) FROM project_board_columns WHERE id = $1 AND tenant_id = $2")
                .bind(column_id).bind(&tenant_id).fetch_optional(&self.pool).await.map_err(db_err)?
        } else {
            None
        };
        sqlx::query(
            "UPDATE work_items
             SET column_id = COALESCE($1, column_id),
                 status = COALESCE($2, status),
                 position = COALESCE($3, position),
                 updated_at = $4
             WHERE id = $5 AND tenant_id = $6",
        )
        .bind(column_id)
        .bind(status)
        .bind(position)
        .bind(now())
        .bind(id)
        .bind(&tenant_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        insert_work_item_event(
            &self.pool,
            &tenant_id,
            id,
            "system",
            None,
            "moved",
            serde_json::json!({}),
        )
        .await?;
        fetch_work_item(&self.pool, id, &tenant_id).await
    }

    async fn reorder(
        &self,
        project_id: &str,
        board_id: Option<String>,
        status: Option<String>,
        column_id: Option<String>,
        ordered_ids: Vec<String>,
    ) -> Result<(), String> {
        let tenant_id = self.tenant_id();
        for (idx, id) in ordered_ids.iter().enumerate() {
            sqlx::query(
                "UPDATE work_items
                 SET position = $1, updated_at = $2
                 WHERE id = $3 AND project_id = $4 AND tenant_id = $5
                   AND ($6::text IS NULL OR board_id = $6)
                   AND ($7::text IS NULL OR status = $7)
                   AND ($8::text IS NULL OR column_id = $8)",
            )
            .bind((idx as f64 + 1.0) * 1024.0)
            .bind(now())
            .bind(id)
            .bind(project_id)
            .bind(&tenant_id)
            .bind(board_id.as_deref())
            .bind(status.as_deref())
            .bind(column_id.as_deref())
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn block(&self, id: &str, reason: String) -> Result<WorkItem, String> {
        let tenant_id = self.tenant_id();
        sqlx::query("UPDATE work_items SET status = 'blocked', blocked_reason = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
            .bind(&reason).bind(now()).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        insert_work_item_event(
            &self.pool,
            &tenant_id,
            id,
            "system",
            None,
            "blocked",
            serde_json::json!({ "reason": reason }),
        )
        .await?;
        fetch_work_item(&self.pool, id, &tenant_id).await
    }

    async fn unblock(&self, id: &str, status: String) -> Result<WorkItem, String> {
        let tenant_id = self.tenant_id();
        sqlx::query("UPDATE work_items SET status = $1, blocked_reason = NULL, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
            .bind(&status).bind(now()).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        insert_work_item_event(
            &self.pool,
            &tenant_id,
            id,
            "system",
            None,
            "unblocked",
            serde_json::json!({ "status": status }),
        )
        .await?;
        fetch_work_item(&self.pool, id, &tenant_id).await
    }

    async fn complete(&self, id: &str) -> Result<WorkItem, String> {
        let tenant_id = self.tenant_id();
        let completed_at = now();
        sqlx::query("UPDATE work_items SET status = 'done', completed_at = $1, updated_at = $1 WHERE id = $2 AND tenant_id = $3")
            .bind(&completed_at).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        insert_work_item_event(
            &self.pool,
            &tenant_id,
            id,
            "system",
            None,
            "completed",
            serde_json::json!({}),
        )
        .await?;
        fetch_work_item(&self.pool, id, &tenant_id).await
    }

    async fn list_comments(&self, work_item_id: &str) -> Result<Vec<WorkItemComment>, String> {
        let rows = sqlx::query(&format!(
            "SELECT {WORK_ITEM_COMMENT_COLUMNS} FROM work_item_comments
             WHERE work_item_id = $1 AND tenant_id = $2 ORDER BY created_at ASC"
        ))
        .bind(work_item_id)
        .bind(self.tenant_id())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.iter()
            .map(map_work_item_comment_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn create_comment(
        &self,
        work_item_id: &str,
        body: String,
        author: CommentAuthor,
    ) -> Result<WorkItemComment, String> {
        let tenant_id = self.tenant_id();
        let id = Ulid::new().to_string();
        let now = now();
        let (author_kind, author_agent_id) = match author {
            CommentAuthor::User => ("user".to_string(), None),
            CommentAuthor::Agent { agent_id } => ("agent".to_string(), Some(agent_id)),
        };
        sqlx::query(
            "INSERT INTO work_item_comments
             (id, work_item_id, author_kind, author_agent_id, body, created_at, updated_at, tenant_id)
             VALUES ($1,$2,$3,$4,$5,$6,$6,$7)",
        )
        .bind(&id).bind(work_item_id).bind(&author_kind).bind(author_agent_id.as_deref())
        .bind(body).bind(&now).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        insert_work_item_event(
            &self.pool,
            &tenant_id,
            work_item_id,
            &author_kind,
            author_agent_id.as_deref(),
            "comment_created",
            serde_json::json!({ "commentId": id }),
        )
        .await?;
        let row = sqlx::query(&format!("SELECT {WORK_ITEM_COMMENT_COLUMNS} FROM work_item_comments WHERE id = $1 AND tenant_id = $2"))
            .bind(&id).bind(&tenant_id).fetch_one(&self.pool).await.map_err(db_err)?;
        map_work_item_comment_row(&row).map_err(db_err)
    }

    async fn update_comment(&self, id: &str, body: String) -> Result<WorkItemComment, String> {
        let tenant_id = self.tenant_id();
        let work_item_id: String = sqlx::query_scalar(
            "SELECT work_item_id FROM work_item_comments WHERE id = $1 AND tenant_id = $2",
        )
        .bind(id)
        .bind(&tenant_id)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        sqlx::query("UPDATE work_item_comments SET body = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
            .bind(body).bind(now()).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        insert_work_item_event(
            &self.pool,
            &tenant_id,
            &work_item_id,
            "system",
            None,
            "comment_updated",
            serde_json::json!({ "commentId": id }),
        )
        .await?;
        let row = sqlx::query(&format!("SELECT {WORK_ITEM_COMMENT_COLUMNS} FROM work_item_comments WHERE id = $1 AND tenant_id = $2"))
            .bind(id).bind(&tenant_id).fetch_one(&self.pool).await.map_err(db_err)?;
        map_work_item_comment_row(&row).map_err(db_err)
    }

    async fn delete_comment(&self, id: &str) -> Result<(), String> {
        let tenant_id = self.tenant_id();
        let work_item_id: String = sqlx::query_scalar(
            "SELECT work_item_id FROM work_item_comments WHERE id = $1 AND tenant_id = $2",
        )
        .bind(id)
        .bind(&tenant_id)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        sqlx::query("DELETE FROM work_item_comments WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(&tenant_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        insert_work_item_event(
            &self.pool,
            &tenant_id,
            &work_item_id,
            "system",
            None,
            "comment_deleted",
            serde_json::json!({ "commentId": id }),
        )
        .await?;
        Ok(())
    }
}

#[async_trait]
impl ProjectWorkflowRepo for PgRepos {
    async fn list(&self, project_id: &str, limit: i64) -> Result<Vec<ProjectWorkflow>, String> {
        let rows = sqlx::query(&format!(
            "SELECT {PROJECT_WORKFLOW_COLUMNS} FROM project_workflows
             WHERE project_id = $1 AND tenant_id = $2 ORDER BY name ASC LIMIT $3"
        ))
        .bind(project_id)
        .bind(self.tenant_id())
        .bind(limit.clamp(1, 200))
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.iter()
            .map(map_project_workflow_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn get(&self, id: &str) -> Result<ProjectWorkflow, String> {
        let row = sqlx::query(&format!(
            "SELECT {PROJECT_WORKFLOW_COLUMNS} FROM project_workflows WHERE id = $1 AND tenant_id = $2"
        ))
        .bind(id).bind(self.tenant_id()).fetch_one(&self.pool).await.map_err(db_err)?;
        map_project_workflow_row(&row).map_err(db_err)
    }

    async fn create(&self, payload: CreateProjectWorkflow) -> Result<ProjectWorkflow, String> {
        let tenant_id = self.tenant_id();
        let id = Ulid::new().to_string();
        let now = now();
        let graph = payload.graph.unwrap_or_default();
        let trigger_kind = payload.trigger_kind.unwrap_or_else(|| "manual".to_string());
        let trigger_config = payload.trigger_config.unwrap_or(serde_json::Value::Null);
        sqlx::query(
            "INSERT INTO project_workflows (
                id, project_id, name, description, enabled, graph, trigger_kind, trigger_config,
                version, created_at, updated_at, tenant_id
             ) VALUES ($1,$2,$3,$4,false,$5,$6,$7,1,$8,$8,$9)",
        )
        .bind(&id)
        .bind(payload.project_id)
        .bind(payload.name)
        .bind(payload.description)
        .bind(serde_json::to_string(&graph).map_err(db_err)?)
        .bind(trigger_kind)
        .bind(json_string(&trigger_config)?)
        .bind(now)
        .bind(&tenant_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        ProjectWorkflowRepo::get(self, &id).await
    }

    async fn update(
        &self,
        id: &str,
        payload: UpdateProjectWorkflow,
    ) -> Result<ProjectWorkflow, String> {
        let tenant_id = self.tenant_id();
        let now = now();
        if let Some(v) = payload.name {
            if v.trim().is_empty() {
                return Err("workflow: name must be non-empty".into());
            }
            sqlx::query("UPDATE project_workflows SET name = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(v).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        if let Some(v) = payload.description {
            sqlx::query("UPDATE project_workflows SET description = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(v).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        if let Some(v) = payload.graph {
            sqlx::query("UPDATE project_workflows SET graph = $1, version = version + 1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(serde_json::to_string(&v).map_err(db_err)?).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        if let Some(v) = payload.trigger_kind {
            sqlx::query("UPDATE project_workflows SET trigger_kind = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(v).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        if let Some(v) = payload.trigger_config {
            sqlx::query("UPDATE project_workflows SET trigger_config = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
                .bind(json_string(&v)?).bind(&now).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        }
        ProjectWorkflowRepo::get(self, id).await
    }

    async fn delete(&self, id: &str) -> Result<(), String> {
        let tenant_id = self.tenant_id();
        sqlx::query("DELETE FROM schedules WHERE workflow_id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(&tenant_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        sqlx::query("DELETE FROM project_workflows WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(&tenant_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn set_enabled(&self, id: &str, enabled: bool) -> Result<ProjectWorkflow, String> {
        let tenant_id = self.tenant_id();
        sqlx::query("UPDATE project_workflows SET enabled = $1, updated_at = $2 WHERE id = $3 AND tenant_id = $4")
            .bind(enabled).bind(now()).bind(id).bind(&tenant_id).execute(&self.pool).await.map_err(db_err)?;
        ProjectWorkflowRepo::get(self, id).await
    }

    async fn lookup_project_id(&self, workflow_id: &str) -> Result<String, String> {
        sqlx::query_scalar(
            "SELECT project_id FROM project_workflows WHERE id = $1 AND tenant_id = $2",
        )
        .bind(workflow_id)
        .bind(self.tenant_id())
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)
    }

    async fn lookup_run_scope(&self, run_id: &str) -> Result<(String, String), String> {
        let row = sqlx::query(
            "SELECT wr.workflow_id, pw.project_id
             FROM workflow_runs wr
             INNER JOIN project_workflows pw ON pw.id = wr.workflow_id AND pw.tenant_id = wr.tenant_id
             WHERE wr.id = $1 AND wr.tenant_id = $2",
        )
        .bind(run_id).bind(self.tenant_id()).fetch_one(&self.pool).await.map_err(db_err)?;
        Ok((
            row.try_get(0).map_err(db_err)?,
            row.try_get(1).map_err(db_err)?,
        ))
    }
}

#[async_trait]
impl WorkflowRunRepo for PgRepos {
    async fn create_run(
        &self,
        workflow: &ProjectWorkflow,
        trigger_kind: &str,
        trigger_data: &serde_json::Value,
    ) -> Result<WorkflowRun, String> {
        let tenant_id = self.tenant_id();
        let id = Ulid::new().to_string();
        let created_at = now();
        let graph_snapshot =
            serde_json::to_value(&workflow.graph).unwrap_or(serde_json::Value::Null);
        let graph_str = serde_json::to_string(&workflow.graph).map_err(db_err)?;
        let trigger_data_value = trigger_data.clone();
        let trigger_data_str = json_string(trigger_data)?;
        sqlx::query(
            "INSERT INTO workflow_runs (
                id, workflow_id, workflow_version, graph_snapshot, trigger_kind, trigger_data,
                status, created_at, tenant_id
             ) VALUES ($1,$2,$3,$4,$5,$6,'queued',$7,$8)",
        )
        .bind(&id)
        .bind(&workflow.id)
        .bind(workflow.version)
        .bind(graph_str)
        .bind(trigger_kind)
        .bind(trigger_data_str)
        .bind(&created_at)
        .bind(&tenant_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(WorkflowRun {
            id,
            workflow_id: workflow.id.clone(),
            workflow_version: workflow.version,
            graph_snapshot,
            trigger_kind: trigger_kind.to_string(),
            trigger_data: trigger_data_value,
            status: "queued".to_string(),
            error: None,
            started_at: None,
            completed_at: None,
            created_at,
        })
    }

    async fn update_status(
        &self,
        workflow_id: &str,
        run_id: &str,
        status: &str,
        error: Option<&str>,
        started_at: Option<&str>,
        completed_at: Option<&str>,
    ) -> Result<(), String> {
        sqlx::query(
            "UPDATE workflow_runs
             SET status = $1,
                 error = COALESCE($2, error),
                 started_at = COALESCE($3, started_at),
                 completed_at = COALESCE($4, completed_at)
             WHERE id = $5 AND workflow_id = $6 AND tenant_id = $7",
        )
        .bind(status)
        .bind(error)
        .bind(started_at)
        .bind(completed_at)
        .bind(run_id)
        .bind(workflow_id)
        .bind(self.tenant_id())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn insert_step(
        &self,
        run_id: &str,
        node_id: &str,
        node_type: &str,
        status: &str,
        input: &serde_json::Value,
        started_at: Option<&str>,
        sequence: i64,
    ) -> Result<WorkflowRunStep, String> {
        let step = WorkflowRunStep {
            id: Ulid::new().to_string(),
            run_id: run_id.to_string(),
            node_id: node_id.to_string(),
            node_type: node_type.to_string(),
            status: status.to_string(),
            input: input.clone(),
            output: None,
            error: None,
            started_at: started_at.map(String::from),
            completed_at: None,
            sequence,
        };
        sqlx::query(
            "INSERT INTO workflow_run_steps (
                id, run_id, node_id, node_type, status, input, started_at, sequence, tenant_id
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(&step.id)
        .bind(&step.run_id)
        .bind(&step.node_id)
        .bind(&step.node_type)
        .bind(&step.status)
        .bind(json_string(input)?)
        .bind(&step.started_at)
        .bind(step.sequence)
        .bind(self.tenant_id())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(step)
    }

    async fn finish_step(
        &self,
        run_id: &str,
        step_id: &str,
        status: &str,
        output: Option<&serde_json::Value>,
        error: Option<&str>,
        completed_at: &str,
    ) -> Result<(), String> {
        let output_str = output.map(json_string).transpose()?;
        sqlx::query(
            "UPDATE workflow_run_steps
             SET status = $1, output = $2, error = $3, completed_at = $4
             WHERE id = $5 AND run_id = $6 AND tenant_id = $7",
        )
        .bind(status)
        .bind(output_str)
        .bind(error)
        .bind(completed_at)
        .bind(step_id)
        .bind(run_id)
        .bind(self.tenant_id())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn list_for_workflow(
        &self,
        workflow_id: &str,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>, String> {
        let rows = sqlx::query(&format!(
            "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs
             WHERE workflow_id = $1 AND tenant_id = $2
             ORDER BY created_at DESC LIMIT $3"
        ))
        .bind(workflow_id)
        .bind(self.tenant_id())
        .bind(limit.clamp(1, 200))
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.iter()
            .map(map_workflow_run_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    async fn list_for_project(
        &self,
        project_id: &str,
        limit: i64,
    ) -> Result<Vec<WorkflowRunSummary>, String> {
        let rows = sqlx::query(
            "SELECT wr.id, wr.workflow_id, pw.name, wr.workflow_version, wr.trigger_kind,
                    wr.status, wr.error, wr.started_at::text, wr.completed_at::text, wr.created_at::text
             FROM workflow_runs wr
             INNER JOIN project_workflows pw ON pw.id = wr.workflow_id AND pw.tenant_id = wr.tenant_id
             WHERE pw.project_id = $1 AND wr.tenant_id = $2
             ORDER BY wr.created_at DESC LIMIT $3",
        )
        .bind(project_id).bind(self.tenant_id()).bind(limit.clamp(1, 200)).fetch_all(&self.pool).await.map_err(db_err)?;
        rows.iter()
            .map(|row| {
                Ok(WorkflowRunSummary {
                    id: row.try_get(0)?,
                    workflow_id: row.try_get(1)?,
                    workflow_name: row.try_get(2)?,
                    workflow_version: row.try_get(3)?,
                    trigger_kind: row.try_get(4)?,
                    status: row.try_get(5)?,
                    error: row.try_get(6)?,
                    started_at: row.try_get(7)?,
                    completed_at: row.try_get(8)?,
                    created_at: row.try_get(9)?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(db_err)
    }

    async fn get_with_steps(&self, run_id: &str) -> Result<WorkflowRunWithSteps, String> {
        let tenant_id = self.tenant_id();
        let run_row = sqlx::query(&format!(
            "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE id = $1 AND tenant_id = $2"
        ))
        .bind(run_id)
        .bind(&tenant_id)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        let run = map_workflow_run_row(&run_row).map_err(db_err)?;
        let rows = sqlx::query(
            "SELECT id, run_id, node_id, node_type, status, input::text, output::text,
                    error, started_at::text, completed_at::text, sequence
             FROM workflow_run_steps
             WHERE run_id = $1 AND tenant_id = $2
             ORDER BY sequence ASC",
        )
        .bind(run_id)
        .bind(&tenant_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        let steps = rows
            .iter()
            .map(|row| {
                let input: String = row.try_get(5)?;
                let output: Option<String> = row.try_get(6)?;
                Ok(WorkflowRunStep {
                    id: row.try_get(0)?,
                    run_id: row.try_get(1)?,
                    node_id: row.try_get(2)?,
                    node_type: row.try_get(3)?,
                    status: row.try_get(4)?,
                    input: parse_json(&input),
                    output: output.as_deref().map(parse_json),
                    error: row.try_get(7)?,
                    started_at: row.try_get(8)?,
                    completed_at: row.try_get(9)?,
                    sequence: row.try_get(10)?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(db_err)?;
        Ok(WorkflowRunWithSteps { run, steps })
    }

    async fn cancel(&self, run_id: &str) -> Result<(), String> {
        let tenant_id = self.tenant_id();
        let completed_at = now();
        sqlx::query(
            "UPDATE workflow_runs
             SET status = 'cancelled', completed_at = $1
             WHERE id = $2 AND tenant_id = $3 AND status IN ('queued','running')",
        )
        .bind(&completed_at)
        .bind(run_id)
        .bind(&tenant_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        sqlx::query(
            "UPDATE workflow_run_steps
             SET status = 'cancelled', completed_at = COALESCE(completed_at, $1)
             WHERE run_id = $2 AND tenant_id = $3 AND status IN ('queued','running')",
        )
        .bind(completed_at)
        .bind(run_id)
        .bind(&tenant_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }
}
