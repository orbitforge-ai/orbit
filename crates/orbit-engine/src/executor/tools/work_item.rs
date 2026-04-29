//! `work_item` agent tool — lets agents manipulate a project's persistent
//! kanban board. Scoped to the calling agent's assigned projects via
//! `assert_agent_in_project`. Distinct from the session-local `task` tool.

use serde_json::{json, Value};

use crate::executor::llm_provider::ToolDefinition;
use crate::models::work_item::{CreateWorkItem, UpdateWorkItem, WorkItem};
use crate::models::work_item_comment::CommentAuthor;
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
            description: "Manipulate the project kanban board. Work items are persistent, project-scoped cards visible to every agent assigned to the project. When a user asks to create, update, or track a task for the project, prefer this tool over the session-local `task` tool. Actions: list, get, create, update, delete, claim, move, block, unblock, complete, comment, list_comments. `project_id` is inferred from the current session when omitted. When moving a card, do not try to change `status` directly: pass `column_id` for an explicit destination, or omit it to advance to the next board column.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "list", "get", "create", "update", "delete", "claim",
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
                    "status": { "type": "string", "enum": ["backlog", "todo", "in_progress", "blocked", "review", "done", "cancelled"], "description": "Status filter for list, or an optional create hint used to resolve the default board column." },
                    "column_id": { "type": "string", "description": "Explicit board column id for create, move, or list filtering. For move, omit this to advance to the next board column." },
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
        let repos = ctx
            .repos
            .as_ref()
            .ok_or("work_item: no repositories available")?;
        let action = input["action"]
            .as_str()
            .ok_or("work_item: missing 'action' field")?;

        match action {
            "list" => {
                let project_id = resolve_project_id(ctx, input, None).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let status = input["status"].as_str().map(String::from);
                let column_id = input["column_id"].as_str().map(String::from);
                let kind = input["kind"].as_str().map(String::from);
                let assignee_raw = input["assignee"].as_str();
                let assignee_filter = assignee_raw.map(|s| match s {
                    "me" => AssigneeFilter::Agent(ctx.agent_id.clone()),
                    "none" | "unassigned" | "null" => AssigneeFilter::Unassigned,
                    other => AssigneeFilter::Agent(other.to_string()),
                });
                let limit = input["limit"].as_i64();
                let items = list_work_items(
                    ctx,
                    &project_id,
                    status,
                    column_id,
                    kind,
                    assignee_filter,
                    limit,
                )
                .await?;
                let result = serde_json::to_string_pretty(&items)
                    .map_err(|e| format!("work_item: serialize: {}", e))?;
                Ok((result, false))
            }
            "get" => {
                let id = required_str(input, "id", "get")?;
                let project_id = resolve_project_for_item(ctx, input, id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let item = repos.work_items().get(id).await?;
                let result = serde_json::to_string_pretty(&item)
                    .map_err(|e| format!("work_item: serialize: {}", e))?;
                Ok((result, false))
            }
            "create" => {
                let project_id = resolve_project_id(ctx, input, None).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let title = required_str(input, "title", "create")?;
                let description = optional_trimmed(input.get("description"));
                let kind = input["kind"].as_str().unwrap_or("task").to_string();
                let priority = input["priority"].as_i64().unwrap_or(0);
                let parent_id = optional_trimmed(input.get("parent_id"));
                let labels = parse_labels(input.get("labels"))?;
                let status = input["status"].as_str().map(String::from);
                let column_id = input["column_id"].as_str().map(String::from);
                let item = repos
                    .work_items()
                    .create(CreateWorkItem {
                        project_id,
                        board_id: None,
                        title: title.to_string(),
                        description,
                        kind: Some(kind),
                        column_id,
                        status,
                        priority: Some(priority),
                        assignee_agent_id: None,
                        created_by_agent_id: Some(ctx.agent_id.clone()),
                        parent_work_item_id: parent_id,
                        position: None,
                        labels: Some(labels),
                        metadata: None,
                    })
                    .await?;
                spawn_cloud_upsert(ctx, &item);
                let result = mutation_result("created", &item)?;
                Ok((result, false))
            }
            "update" => {
                let id = required_str(input, "id", "update")?;
                let project_id = resolve_project_for_item(ctx, input, id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let title = optional_trimmed(input.get("title"));
                let description = optional_trimmed(input.get("description"));
                let kind = input["kind"].as_str().map(String::from);
                let priority = input["priority"].as_i64();
                let labels = match input.get("labels") {
                    Some(v) if !v.is_null() => Some(parse_labels(Some(v))?),
                    _ => None,
                };
                let item = repos
                    .work_items()
                    .update(
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
                    .await?;
                spawn_cloud_upsert(ctx, &item);
                let result = mutation_result("updated", &item)?;
                Ok((result, false))
            }
            "delete" => {
                let id = required_str(input, "id", "delete")?;
                let project_id = resolve_project_for_item(ctx, input, id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                repos.work_items().delete(id).await?;
                spawn_cloud_delete(ctx, id);
                let result = serde_json::to_string_pretty(&json!({
                    "status": "deleted",
                    "id": id,
                    "project_id": project_id,
                }))
                .map_err(|e| format!("work_item: serialize: {}", e))?;
                Ok((result, false))
            }
            "claim" => {
                let id = required_str(input, "id", "claim")?;
                let project_id = resolve_project_for_item(ctx, input, id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let item = repos.work_items().claim(id, &ctx.agent_id).await?;
                spawn_cloud_upsert(ctx, &item);
                let result = mutation_result("claimed", &item)?;
                Ok((result, false))
            }
            "move" => {
                let id = required_str(input, "id", "move")?;
                let project_id = resolve_project_for_item(ctx, input, id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                if input.get("status").is_some_and(|value| !value.is_null()) {
                    return Err(
                        "work_item: move no longer accepts 'status'; pass 'column_id' or omit it to advance to the next column".into(),
                    );
                }
                let column_id = input["column_id"].as_str().map(str::to_string);
                let item = repos.work_items().move_item(id, column_id, None).await?;
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
                let project_id = resolve_project_for_item(ctx, input, id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let item = repos.work_items().block(id, reason).await?;
                spawn_cloud_upsert(ctx, &item);
                let result = mutation_result("blocked", &item)?;
                Ok((result, false))
            }
            "unblock" => {
                let id = required_str(input, "id", "unblock")?;
                let project_id = resolve_project_for_item(ctx, input, id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let new_status = input["new_status"].as_str().unwrap_or("todo").to_string();
                if !matches!(
                    new_status.as_str(),
                    "backlog" | "todo" | "in_progress" | "review"
                ) {
                    return Err("work_item: unblock new_status must be one of backlog/todo/in_progress/review".into());
                }
                let item = repos.work_items().unblock(id, new_status).await?;
                spawn_cloud_upsert(ctx, &item);
                let result = mutation_result("unblocked", &item)?;
                Ok((result, false))
            }
            "complete" => {
                let id = required_str(input, "id", "complete")?;
                let project_id = resolve_project_for_item(ctx, input, id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let item = repos.work_items().complete(id).await?;
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
                let project_id = resolve_project_for_item(ctx, input, id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let comment = repos
                    .work_items()
                    .create_comment(
                        id,
                        body,
                        CommentAuthor::Agent {
                            agent_id: ctx.agent_id.clone(),
                        },
                    )
                    .await?;
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
                let project_id = resolve_project_for_item(ctx, input, id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let comments = repos.work_items().list_comments(id).await?;
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
    let repos = ctx
        .repos
        .as_ref()
        .ok_or("work_item: no repositories available")?;
    let project_id = repos.chat().session_meta(session_id).await?.project_id;
    project_id.ok_or_else(|| {
        "work_item: no project_id provided and current session is not scoped to a project"
            .to_string()
    })
}

/// Look up the item's project_id from the DB — used by per-item actions so
/// the agent doesn't have to pass `project_id` explicitly when referencing a
/// known work item. Still runs `assert_agent_in_project` afterward.
async fn resolve_project_for_item(
    ctx: &ToolExecutionContext,
    input: &Value,
    id: &str,
) -> Result<String, String> {
    if let Some(p) = input["project_id"].as_str() {
        if !p.is_empty() {
            return Ok(p.to_string());
        }
    }
    let repos = ctx
        .repos
        .as_ref()
        .ok_or("work_item: no repositories available")?;
    repos.work_items().lookup_project_id(id).await
}

async fn enforce_project_scope(ctx: &ToolExecutionContext, project_id: &str) -> Result<(), String> {
    let repos = ctx
        .repos
        .as_ref()
        .ok_or("work_item: no repositories available")?;
    let in_project = repos
        .projects()
        .agent_in_project(project_id, &ctx.agent_id)
        .await?;
    if !in_project {
        return Err(serde_json::json!({
            "code": "agent_not_in_project",
            "project_id": project_id,
            "message": format!("agent is not a member of project '{}'", project_id),
        })
        .to_string());
    }
    Ok(())
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

fn spawn_cloud_delete(ctx: &ToolExecutionContext, id: &str) {
    if let Some(client) = ctx.cloud_client.clone() {
        let id = id.to_string();
        tokio::spawn(async move {
            if let Err(e) = client.delete_by_id("work_items", &id).await {
                tracing::warn!("cloud delete work_item (tool): {}", e);
            }
        });
    }
}

async fn list_work_items(
    ctx: &ToolExecutionContext,
    project_id: &str,
    status: Option<String>,
    column_id: Option<String>,
    kind: Option<String>,
    assignee: Option<AssigneeFilter>,
    limit: Option<i64>,
) -> Result<Vec<WorkItem>, String> {
    let repos = ctx
        .repos
        .as_ref()
        .ok_or("work_item: no repositories available")?;
    let mut items = repos.work_items().list(project_id, None).await?;
    if let Some(column_id) = column_id {
        items.retain(|item| item.column_id.as_deref() == Some(column_id.as_str()));
    }
    if let Some(status) = status {
        items.retain(|item| item.status == status);
    }
    if let Some(kind) = kind {
        items.retain(|item| item.kind == kind);
    }
    match assignee {
        Some(AssigneeFilter::Agent(agent_id)) => {
            items.retain(|item| item.assignee_agent_id.as_deref() == Some(agent_id.as_str()));
        }
        Some(AssigneeFilter::Unassigned) => {
            items.retain(|item| item.assignee_agent_id.is_none());
        }
        None => {}
    }
    if let Some(limit) = limit.and_then(|value| usize::try_from(value.max(0)).ok()) {
        items.truncate(limit);
    }
    Ok(items)
}
