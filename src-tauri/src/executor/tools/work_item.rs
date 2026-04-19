//! `work_item` agent tool — lets agents manipulate a project's persistent
//! kanban board. Scoped to the calling agent's assigned projects via
//! `assert_agent_in_project`. Distinct from the session-local `task` tool.

use serde_json::{json, Value};
use ulid::Ulid;

use crate::commands::work_items::{
    assert_agent_in_project, block_work_item_with_db, claim_work_item_with_db,
    complete_work_item_with_db, create_work_item_with_db, fetch_work_item_project,
    move_work_item_with_db, unblock_work_item_with_db, update_work_item_with_db, WorkItemError,
};
use crate::db::DbPool;
use crate::executor::llm_provider::ToolDefinition;
use crate::models::work_item::{CreateWorkItem, UpdateWorkItem, WorkItem};
use crate::models::work_item_comment::WorkItemComment;

use super::{context::ToolExecutionContext, ToolHandler};

pub struct WorkItemTool;

#[async_trait::async_trait]
impl ToolHandler for WorkItemTool {
    fn name(&self) -> &'static str {
        "work_item"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Manipulate the project kanban board. Work items are persistent, project-scoped cards visible to every agent assigned to the project. When a user asks to create, update, or track a task for the project, prefer this tool over the session-local `task` tool. Actions: list, get, create, update, claim, move, block, unblock, complete, comment, list_comments. `project_id` is inferred from the current session when omitted. `review` means 'another agent should verify'; `in_progress` means 'I am actively on it'.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "list", "get", "create", "update", "claim",
                            "move", "block", "unblock", "complete",
                            "comment", "list_comments"
                        ],
                        "description": "Action to perform"
                    },
                    "project_id": { "type": "string", "description": "Target project. Defaults to the current session's project when omitted." },
                    "id": { "type": "string", "description": "Work item ID (required for per-item actions)." },
                    "title": { "type": "string", "description": "Card title (create/update)." },
                    "description": { "type": "string", "description": "Markdown body (create/update)." },
                    "kind": { "type": "string", "enum": ["task", "bug", "story", "spike", "chore"], "description": "Card kind." },
                    "priority": { "type": "integer", "minimum": 0, "maximum": 3, "description": "0 (low) .. 3 (urgent)." },
                    "status": { "type": "string", "enum": ["backlog", "todo", "in_progress", "blocked", "review", "done", "cancelled"], "description": "Status filter (list) or target status (move)." },
                    "assignee": { "type": "string", "description": "Filter by assignee agent id when listing. Use 'me' for self, or 'none' for unassigned." },
                    "parent_id": { "type": "string", "description": "Parent work item id (for subtasks)." },
                    "labels": { "type": "array", "items": { "type": "string" }, "description": "Labels (create/update)." },
                    "reason": { "type": "string", "description": "Required when action = 'block'." },
                    "body": { "type": "string", "description": "Comment body (comment action)." },
                    "new_status": { "type": "string", "enum": ["backlog", "todo", "in_progress", "review"], "description": "Target status when unblocking. Defaults to 'todo'." },
                    "limit": { "type": "integer", "description": "Max rows to return from list." }
                },
                "required": ["action"]
            }),
        }
    }

    async fn execute(
        &self,
        ctx: &ToolExecutionContext,
        input: &Value,
        _app: &tauri::AppHandle,
        _run_id: &str,
    ) -> Result<(String, bool), String> {
        let db = ctx.db.as_ref().ok_or("work_item: no database available")?;
        let action = input["action"]
            .as_str()
            .ok_or("work_item: missing 'action' field")?;

        match action {
            "list" => {
                let project_id = resolve_project_id(ctx, input, None).await?;
                enforce_project_scope(db, &ctx.agent_id, &project_id).await?;
                let status = input["status"].as_str().map(String::from);
                let kind = input["kind"].as_str().map(String::from);
                let assignee_raw = input["assignee"].as_str();
                let assignee_filter = assignee_raw.map(|s| match s {
                    "me" => AssigneeFilter::Agent(ctx.agent_id.clone()),
                    "none" | "unassigned" | "null" => AssigneeFilter::Unassigned,
                    other => AssigneeFilter::Agent(other.to_string()),
                });
                let limit = input["limit"].as_i64();
                let items =
                    list_work_items(db, &project_id, status, kind, assignee_filter, limit).await?;
                let result = serde_json::to_string_pretty(&items)
                    .map_err(|e| format!("work_item: serialize: {}", e))?;
                Ok((result, false))
            }
            "get" => {
                let id = required_str(input, "id", "get")?;
                let project_id = resolve_project_for_item(db, &ctx.agent_id, input, id).await?;
                enforce_project_scope(db, &ctx.agent_id, &project_id).await?;
                let item = get_work_item(db, id).await?;
                let result = serde_json::to_string_pretty(&item)
                    .map_err(|e| format!("work_item: serialize: {}", e))?;
                Ok((result, false))
            }
            "create" => {
                let project_id = resolve_project_id(ctx, input, None).await?;
                enforce_project_scope(db, &ctx.agent_id, &project_id).await?;
                let title = required_str(input, "title", "create")?;
                let description = optional_trimmed(input.get("description"));
                let kind = input["kind"].as_str().unwrap_or("task").to_string();
                let priority = input["priority"].as_i64().unwrap_or(0);
                let parent_id = optional_trimmed(input.get("parent_id"));
                let labels = parse_labels(input.get("labels"))?;
                let item = create_work_item(
                    db,
                    &project_id,
                    title,
                    description,
                    kind,
                    priority,
                    parent_id,
                    labels,
                    Some(ctx.agent_id.clone()),
                )
                .await?;
                spawn_cloud_upsert(ctx, &item);
                let result = mutation_result("created", &item)?;
                Ok((result, false))
            }
            "update" => {
                let id = required_str(input, "id", "update")?;
                let project_id = resolve_project_for_item(db, &ctx.agent_id, input, id).await?;
                enforce_project_scope(db, &ctx.agent_id, &project_id).await?;
                let title = optional_trimmed(input.get("title"));
                let description = optional_trimmed(input.get("description"));
                let kind = input["kind"].as_str().map(String::from);
                let priority = input["priority"].as_i64();
                let labels = match input.get("labels") {
                    Some(v) if !v.is_null() => Some(parse_labels(Some(v))?),
                    _ => None,
                };
                let item =
                    update_work_item(db, id, title, description, kind, priority, labels).await?;
                spawn_cloud_upsert(ctx, &item);
                let result = mutation_result("updated", &item)?;
                Ok((result, false))
            }
            "claim" => {
                let id = required_str(input, "id", "claim")?;
                let project_id = resolve_project_for_item(db, &ctx.agent_id, input, id).await?;
                enforce_project_scope(db, &ctx.agent_id, &project_id).await?;
                let item = claim_work_item(db, id, &ctx.agent_id).await?;
                spawn_cloud_upsert(ctx, &item);
                let result = mutation_result("claimed", &item)?;
                Ok((result, false))
            }
            "move" => {
                let id = required_str(input, "id", "move")?;
                let project_id = resolve_project_for_item(db, &ctx.agent_id, input, id).await?;
                enforce_project_scope(db, &ctx.agent_id, &project_id).await?;
                let status = required_str(input, "status", "move")?.to_string();
                if status == "blocked" {
                    return Err(
                        "work_item: use action='block' with a reason to move to 'blocked'".into(),
                    );
                }
                let item = move_work_item(db, id, status).await?;
                spawn_cloud_upsert(ctx, &item);
                let result = mutation_result("moved", &item)?;
                Ok((result, false))
            }
            "block" => {
                let id = required_str(input, "id", "block")?;
                let reason = required_str(input, "reason", "block")?.to_string();
                if reason.trim().is_empty() {
                    return Err("work_item: block requires a non-empty 'reason'".into());
                }
                let project_id = resolve_project_for_item(db, &ctx.agent_id, input, id).await?;
                enforce_project_scope(db, &ctx.agent_id, &project_id).await?;
                let item = block_work_item(db, id, reason).await?;
                spawn_cloud_upsert(ctx, &item);
                let result = mutation_result("blocked", &item)?;
                Ok((result, false))
            }
            "unblock" => {
                let id = required_str(input, "id", "unblock")?;
                let project_id = resolve_project_for_item(db, &ctx.agent_id, input, id).await?;
                enforce_project_scope(db, &ctx.agent_id, &project_id).await?;
                let new_status = input["new_status"].as_str().unwrap_or("todo").to_string();
                if !matches!(
                    new_status.as_str(),
                    "backlog" | "todo" | "in_progress" | "review"
                ) {
                    return Err("work_item: unblock new_status must be one of backlog/todo/in_progress/review".into());
                }
                let item = unblock_work_item(db, id, new_status).await?;
                spawn_cloud_upsert(ctx, &item);
                let result = mutation_result("unblocked", &item)?;
                Ok((result, false))
            }
            "complete" => {
                let id = required_str(input, "id", "complete")?;
                let project_id = resolve_project_for_item(db, &ctx.agent_id, input, id).await?;
                enforce_project_scope(db, &ctx.agent_id, &project_id).await?;
                let item = complete_work_item(db, id).await?;
                spawn_cloud_upsert(ctx, &item);
                let result = mutation_result("completed", &item)?;
                Ok((result, false))
            }
            "comment" => {
                let id = required_str(input, "id", "comment")?;
                let body = required_str(input, "body", "comment")?.to_string();
                if body.trim().is_empty() {
                    return Err("work_item: comment requires a non-empty 'body'".into());
                }
                let project_id = resolve_project_for_item(db, &ctx.agent_id, input, id).await?;
                enforce_project_scope(db, &ctx.agent_id, &project_id).await?;
                let comment = create_comment(db, id, &ctx.agent_id, body).await?;
                spawn_cloud_upsert_comment(ctx, &comment);
                let result = serde_json::to_string_pretty(&json!({
                    "status": "commented",
                    "comment": comment,
                }))
                .map_err(|e| format!("work_item: serialize: {}", e))?;
                Ok((result, false))
            }
            "list_comments" => {
                let id = required_str(input, "id", "list_comments")?;
                let project_id = resolve_project_for_item(db, &ctx.agent_id, input, id).await?;
                enforce_project_scope(db, &ctx.agent_id, &project_id).await?;
                let comments = list_comments(db, id).await?;
                let result = serde_json::to_string_pretty(&comments)
                    .map_err(|e| format!("work_item: serialize: {}", e))?;
                Ok((result, false))
            }
            other => Err(format!("work_item: unknown action '{}'", other)),
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum AssigneeFilter {
    Agent(String),
    Unassigned,
}

fn required_str<'a>(input: &'a Value, field: &str, action: &str) -> Result<&'a str, String> {
    input[field]
        .as_str()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("work_item: {} requires '{}'", action, field))
}

fn optional_trimmed(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn parse_labels(value: Option<&Value>) -> Result<Vec<String>, String> {
    let Some(v) = value else { return Ok(vec![]) };
    if v.is_null() {
        return Ok(vec![]);
    }
    let arr = v
        .as_array()
        .ok_or("work_item: 'labels' must be an array of strings")?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let s = item
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("work_item: label entries must be non-empty strings")?;
        out.push(s.to_string());
    }
    Ok(out)
}

fn mutation_result(status: &str, item: &WorkItem) -> Result<String, String> {
    serde_json::to_string_pretty(&json!({
        "status": status,
        "work_item": item,
    }))
    .map_err(|e| format!("work_item: serialize: {}", e))
}

fn map_error(err: WorkItemError) -> String {
    match err {
        WorkItemError::AgentNotInProject { project_id } => {
            // Structured error the LLM can react to: call list_projects or ask
            // the user instead of looping.
            serde_json::json!({
                "code": "agent_not_in_project",
                "project_id": project_id,
                "message": format!("agent is not a member of project '{}'", project_id),
            })
            .to_string()
        }
        WorkItemError::NotFound(msg) => msg,
        WorkItemError::Other(msg) => msg,
    }
}

async fn resolve_project_id(
    ctx: &ToolExecutionContext,
    input: &Value,
    explicit: Option<&str>,
) -> Result<String, String> {
    if let Some(p) = explicit {
        return Ok(p.to_string());
    }
    if let Some(s) = input["project_id"].as_str() {
        if !s.is_empty() {
            return Ok(s.to_string());
        }
    }
    // Fallback: derive from the current session's project_id column.
    let Some(session_id) = ctx.current_session_id.as_deref() else {
        return Err(
            "work_item: no project_id provided and no current session to infer from".into(),
        );
    };
    let db = ctx.db.as_ref().ok_or("work_item: no database available")?;
    let pool = db.0.clone();
    let session_id = session_id.to_string();
    let project_id: Option<String> =
        tokio::task::spawn_blocking(move || -> Result<Option<String>, String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            conn.query_row(
                "SELECT project_id FROM chat_sessions WHERE id = ?1",
                rusqlite::params![session_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??;
    project_id.ok_or_else(|| {
        "work_item: no project_id provided and current session is not scoped to a project"
            .to_string()
    })
}

/// Look up the item's project_id from the DB — used by per-item actions so
/// the agent doesn't have to pass `project_id` explicitly when referencing a
/// known work item. Still runs `assert_agent_in_project` afterward.
async fn resolve_project_for_item(
    db: &DbPool,
    _agent_id: &str,
    input: &Value,
    id: &str,
) -> Result<String, String> {
    if let Some(p) = input["project_id"].as_str() {
        if !p.is_empty() {
            return Ok(p.to_string());
        }
    }
    let pool = db.0.clone();
    let id = id.to_string();
    tokio::task::spawn_blocking(move || -> Result<String, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        fetch_work_item_project(&conn, &id).map_err(map_error)
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn enforce_project_scope(
    db: &DbPool,
    agent_id: &str,
    project_id: &str,
) -> Result<(), String> {
    let pool = db.0.clone();
    let agent_id = agent_id.to_string();
    let project_id = project_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        assert_agent_in_project(&conn, &agent_id, &project_id).map_err(map_error)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn spawn_cloud_upsert(ctx: &ToolExecutionContext, item: &WorkItem) {
    if let Some(client) = ctx.cloud_client.clone() {
        let item = item.clone();
        tokio::spawn(async move {
            if let Err(e) = client.upsert_work_item(&item).await {
                tracing::warn!("cloud upsert work_item (tool): {}", e);
            }
        });
    }
}

fn spawn_cloud_upsert_comment(ctx: &ToolExecutionContext, comment: &WorkItemComment) {
    if let Some(client) = ctx.cloud_client.clone() {
        let comment = comment.clone();
        tokio::spawn(async move {
            if let Err(e) = client.upsert_work_item_comment(&comment).await {
                tracing::warn!("cloud upsert work_item_comment (tool): {}", e);
            }
        });
    }
}

// ── DB operations (async wrappers that reuse the command helpers) ────────────

const WORK_ITEM_COLUMNS: &str = "id, project_id, title, description, kind, column_id, status, priority,
        assignee_agent_id, created_by_agent_id, parent_work_item_id, position,
        labels, metadata, blocked_reason, started_at, completed_at, created_at, updated_at";

const WORK_ITEM_COMMENT_COLUMNS: &str =
    "id, work_item_id, author_kind, author_agent_id, body, created_at, updated_at";

async fn list_work_items(
    db: &DbPool,
    project_id: &str,
    status: Option<String>,
    kind: Option<String>,
    assignee: Option<AssigneeFilter>,
    limit: Option<i64>,
) -> Result<Vec<WorkItem>, String> {
    let pool = db.0.clone();
    let project_id = project_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<Vec<WorkItem>, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let mut sql = format!(
            "SELECT {} FROM work_items WHERE project_id = ?",
            WORK_ITEM_COLUMNS
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(project_id.clone())];
        if let Some(s) = status {
            sql.push_str(" AND status = ?");
            params.push(Box::new(s));
        }
        if let Some(k) = kind {
            sql.push_str(" AND kind = ?");
            params.push(Box::new(k));
        }
        match assignee {
            Some(AssigneeFilter::Agent(a)) => {
                sql.push_str(" AND assignee_agent_id = ?");
                params.push(Box::new(a));
            }
            Some(AssigneeFilter::Unassigned) => {
                sql.push_str(" AND assignee_agent_id IS NULL");
            }
            None => {}
        }
        sql.push_str(" ORDER BY COALESCE(column_id, status), position ASC");
        if let Some(l) = limit {
            sql.push_str(" LIMIT ?");
            params.push(Box::new(l));
        }
        let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let items = stmt
            .query_map(
                rusqlite::params_from_iter(params_ref.iter()),
                crate::commands::work_items::map_work_item,
            )
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(items)
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn get_work_item(db: &DbPool, id: &str) -> Result<WorkItem, String> {
    let pool = db.0.clone();
    let id = id.to_string();
    tokio::task::spawn_blocking(move || -> Result<WorkItem, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let sql = format!("SELECT {} FROM work_items WHERE id = ?1", WORK_ITEM_COLUMNS);
        conn.query_row(
            &sql,
            rusqlite::params![id],
            crate::commands::work_items::map_work_item,
        )
        .map_err(|e| format!("work_item: not found ({})", e))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[allow(clippy::too_many_arguments)]
async fn create_work_item(
    db: &DbPool,
    project_id: &str,
    title: &str,
    description: Option<String>,
    kind: String,
    priority: i64,
    parent_id: Option<String>,
    labels: Vec<String>,
    created_by_agent_id: Option<String>,
) -> Result<WorkItem, String> {
    let project_id = project_id.to_string();
    let title = title.to_string();
    create_work_item_with_db(
        db,
        CreateWorkItem {
            project_id,
            title,
            description,
            kind: Some(kind),
            column_id: None,
            status: Some("backlog".to_string()),
            priority: Some(priority),
            assignee_agent_id: None,
            created_by_agent_id,
            parent_work_item_id: parent_id,
            position: None,
            labels: Some(labels),
            metadata: None,
        },
    )
    .await
}

async fn update_work_item(
    db: &DbPool,
    id: &str,
    title: Option<String>,
    description: Option<String>,
    kind: Option<String>,
    priority: Option<i64>,
    labels: Option<Vec<String>>,
) -> Result<WorkItem, String> {
    let id = id.to_string();
    update_work_item_with_db(
        db,
        id,
        UpdateWorkItem {
          title,
          description,
          kind,
          column_id: None,
          priority,
          labels,
          metadata: None,
        },
    )
    .await
}

async fn claim_work_item(db: &DbPool, id: &str, agent_id: &str) -> Result<WorkItem, String> {
    claim_work_item_with_db(db, id.to_string(), agent_id.to_string()).await
}

async fn move_work_item(db: &DbPool, id: &str, status: String) -> Result<WorkItem, String> {
    move_work_item_with_db(db, id.to_string(), Some(status), None, None).await
}

async fn block_work_item(db: &DbPool, id: &str, reason: String) -> Result<WorkItem, String> {
    block_work_item_with_db(db, id.to_string(), reason).await
}

async fn unblock_work_item(db: &DbPool, id: &str, new_status: String) -> Result<WorkItem, String> {
    unblock_work_item_with_db(db, id.to_string(), new_status).await
}

async fn complete_work_item(db: &DbPool, id: &str) -> Result<WorkItem, String> {
    complete_work_item_with_db(db, id.to_string()).await
}

async fn create_comment(
    db: &DbPool,
    work_item_id: &str,
    agent_id: &str,
    body: String,
) -> Result<WorkItemComment, String> {
    let pool = db.0.clone();
    let work_item_id = work_item_id.to_string();
    let agent_id = agent_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<WorkItemComment, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let id = Ulid::new().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO work_item_comments (
                id, work_item_id, author_kind, author_agent_id, body, created_at, updated_at
             ) VALUES (?1,?2,'agent',?3,?4,?5,?5)",
            rusqlite::params![id, work_item_id, agent_id, body, now],
        )
        .map_err(|e| e.to_string())?;
        let sql = format!(
            "SELECT {} FROM work_item_comments WHERE id = ?1",
            WORK_ITEM_COMMENT_COLUMNS
        );
        conn.query_row(&sql, rusqlite::params![id], |row| {
            Ok(WorkItemComment {
                id: row.get(0)?,
                work_item_id: row.get(1)?,
                author_kind: row.get(2)?,
                author_agent_id: row.get(3)?,
                body: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn list_comments(db: &DbPool, work_item_id: &str) -> Result<Vec<WorkItemComment>, String> {
    let pool = db.0.clone();
    let work_item_id = work_item_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<Vec<WorkItemComment>, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let sql = format!(
            "SELECT {} FROM work_item_comments WHERE work_item_id = ?1 ORDER BY created_at ASC",
            WORK_ITEM_COMMENT_COLUMNS
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let comments = stmt
            .query_map(rusqlite::params![work_item_id], |row| {
                Ok(WorkItemComment {
                    id: row.get(0)?,
                    work_item_id: row.get(1)?,
                    author_kind: row.get(2)?,
                    author_agent_id: row.get(3)?,
                    body: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(comments)
    })
    .await
    .map_err(|e| e.to_string())?
}
