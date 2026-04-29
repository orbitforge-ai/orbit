use serde::Serialize;
use serde_json::{json, Value};
use ulid::Ulid;

use crate::db::DbPool;
use crate::executor::llm_provider::ToolDefinition;
use crate::models::agent_task::AgentTask;

use super::{context::ToolExecutionContext, ToolHandler};

const VALID_STATUSES: &[&str] = &["pending", "in_progress", "completed"];

pub struct TaskTool;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskMutationResult {
    status: String,
    task: AgentTask,
}

#[async_trait::async_trait]
impl ToolHandler for TaskTool {
    fn name(&self) -> &'static str {
        "task"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Track scratch-pad tasks for the current session only. Use this for the agent's own internal plan when breaking work into smaller steps. These tasks are not project board cards and do not show up on the kanban board; use `work_item` for user-requested tasks or any persistent project work. Actions: create, list, get, update, delete.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["create", "list", "get", "update", "delete"],
                        "description": "Action to perform"
                    },
                    "subject": {
                        "type": "string",
                        "description": "Task subject/title for create"
                    },
                    "description": {
                        "type": "string",
                        "description": "Task description for create or update"
                    },
                    "task_id": {
                        "type": "string",
                        "description": "Task ID for get, update, or delete"
                    },
                    "status": {
                        "type": "string",
                        "enum": ["pending", "in_progress", "completed"],
                        "description": "Task status for update"
                    },
                    "active_form": {
                        "type": "string",
                        "description": "Present-tense activity label shown in the UI"
                    },
                    "blocked_by": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Task IDs that block this task"
                    }
                },
                "required": ["action"]
            }),
        }
    }

    async fn execute(
        &self,
        ctx: &ToolExecutionContext,
        input: &serde_json::Value,
        _app: &tauri::AppHandle,
        _run_id: &str,
    ) -> Result<(String, bool), String> {
        let db = ctx.db.as_ref().ok_or("task: no database available")?;
        let session_id = ctx
            .current_session_id
            .as_deref()
            .ok_or("task: no current session available")?;
        let action = input["action"]
            .as_str()
            .ok_or("task: missing 'action' field")?;

        match action {
            "create" => {
                let subject = input["subject"]
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or("task: create requires a non-empty 'subject'")?;
                let description = optional_trimmed_string(input.get("description"));
                let active_form = optional_trimmed_string(input.get("active_form"));
                let blocked_by = parse_blocked_by(input.get("blocked_by"))?;
                ensure_blocked_by_exists(db, session_id, &ctx.agent_id, &blocked_by).await?;

                let task = create_agent_task(
                    db,
                    session_id,
                    &ctx.agent_id,
                    subject,
                    description,
                    active_form,
                    blocked_by,
                )
                .await?;
                let result = serde_json::to_string_pretty(&TaskMutationResult {
                    status: "created".to_string(),
                    task,
                })
                .map_err(|e| format!("task: failed to serialize result: {}", e))?;
                Ok((result, false))
            }
            "list" => {
                let tasks = list_agent_tasks(db, session_id, &ctx.agent_id).await?;
                let result = serde_json::to_string_pretty(&tasks)
                    .map_err(|e| format!("task: failed to serialize result: {}", e))?;
                Ok((result, false))
            }
            "get" => {
                let task_id = required_task_id(input, "get")?;
                let task = get_agent_task(db, session_id, &ctx.agent_id, task_id).await?;
                let result = serde_json::to_string_pretty(&task)
                    .map_err(|e| format!("task: failed to serialize result: {}", e))?;
                Ok((result, false))
            }
            "update" => {
                let task_id = required_task_id(input, "update")?;
                if let Some(status) = input.get("status").and_then(Value::as_str) {
                    ensure_valid_status(status)?;
                }
                let blocked_by = input
                    .get("blocked_by")
                    .map(|value| parse_blocked_by(Some(value)))
                    .transpose()?;
                if let Some(ref blocked_by) = blocked_by {
                    if blocked_by.iter().any(|blocked| blocked == task_id) {
                        return Err("task: a task cannot be blocked by itself".to_string());
                    }
                    ensure_blocked_by_exists(db, session_id, &ctx.agent_id, blocked_by).await?;
                }

                let task = update_agent_task(
                    db,
                    session_id,
                    &ctx.agent_id,
                    task_id,
                    input.get("subject"),
                    input.get("description"),
                    input.get("status"),
                    input.get("active_form"),
                    blocked_by.as_ref(),
                )
                .await?;
                let result = serde_json::to_string_pretty(&TaskMutationResult {
                    status: "updated".to_string(),
                    task,
                })
                .map_err(|e| format!("task: failed to serialize result: {}", e))?;
                Ok((result, false))
            }
            "delete" => {
                let task_id = required_task_id(input, "delete")?;
                let deleted = delete_agent_task(db, session_id, &ctx.agent_id, task_id).await?;
                let result = serde_json::to_string_pretty(&json!({
                    "status": "deleted",
                    "task": deleted,
                }))
                .map_err(|e| format!("task: failed to serialize result: {}", e))?;
                Ok((result, false))
            }
            other => Err(format!("task: unknown action '{}'", other)),
        }
    }
}

fn required_task_id<'a>(input: &'a Value, action: &str) -> Result<&'a str, String> {
    input["task_id"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("task: {} requires 'task_id'", action))
}

fn optional_trimmed_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn parse_blocked_by(value: Option<&Value>) -> Result<Vec<String>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or("task: blocked_by must be an array of task IDs")?;
    let mut blocked_by = Vec::new();
    for item in array {
        let task_id = item
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or("task: blocked_by entries must be non-empty strings")?;
        if !blocked_by.iter().any(|existing| existing == task_id) {
            blocked_by.push(task_id.to_string());
        }
    }
    Ok(blocked_by)
}

fn ensure_valid_status(status: &str) -> Result<(), String> {
    if VALID_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(format!(
            "task: invalid status '{}'; expected one of pending, in_progress, completed",
            status
        ))
    }
}

fn parse_agent_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentTask> {
    let blocked_by_json: String = row.get(7)?;
    let metadata_json: String = row.get(8)?;
    Ok(AgentTask {
        id: row.get(0)?,
        session_id: row.get(1)?,
        agent_id: row.get(2)?,
        subject: row.get(3)?,
        description: row.get(4)?,
        status: row.get(5)?,
        active_form: row.get(6)?,
        blocked_by: serde_json::from_str(&blocked_by_json).unwrap_or_default(),
        metadata: serde_json::from_str(&metadata_json).unwrap_or_else(|_| json!({})),
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

async fn create_agent_task(
    db: &DbPool,
    session_id: &str,
    agent_id: &str,
    subject: &str,
    description: Option<String>,
    active_form: Option<String>,
    blocked_by: Vec<String>,
) -> Result<AgentTask, String> {
    let pool = db.0.clone();
    let task_id = Ulid::new().to_string();
    let session_id = session_id.to_string();
    let agent_id = agent_id.to_string();
    let subject = subject.to_string();
    tokio::task::spawn_blocking(move || -> Result<AgentTask, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        let blocked_by_json = serde_json::to_string(&blocked_by).map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO agent_tasks (
                id, session_id, agent_id, subject, description, status, active_form,
                blocked_by, metadata, created_at, updated_at, tenant_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?7, '{}', ?8, ?8, COALESCE((SELECT tenant_id FROM chat_sessions WHERE id = ?2), 'local'))",
            rusqlite::params![
                task_id,
                session_id,
                agent_id,
                subject,
                description,
                active_form,
                blocked_by_json,
                now
            ],
        )
        .map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT id, session_id, agent_id, subject, description, status, active_form,
                    blocked_by, metadata, created_at, updated_at
             FROM agent_tasks WHERE id = ?1",
            rusqlite::params![task_id],
            parse_agent_task,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn list_agent_tasks(
    db: &DbPool,
    session_id: &str,
    agent_id: &str,
) -> Result<Vec<AgentTask>, String> {
    let pool = db.0.clone();
    let session_id = session_id.to_string();
    let agent_id = agent_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<Vec<AgentTask>, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, session_id, agent_id, subject, description, status, active_form,
                        blocked_by, metadata, created_at, updated_at
                 FROM agent_tasks
                 WHERE session_id = ?1 AND agent_id = ?2
                 ORDER BY
                    CASE status
                        WHEN 'in_progress' THEN 0
                        WHEN 'pending' THEN 1
                        ELSE 2
                    END,
                    created_at ASC",
            )
            .map_err(|e| e.to_string())?;
        let tasks = stmt
            .query_map(rusqlite::params![session_id, agent_id], parse_agent_task)
            .map_err(|e| e.to_string())?
            .filter_map(|row| row.ok())
            .collect();
        Ok(tasks)
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn get_agent_task(
    db: &DbPool,
    session_id: &str,
    agent_id: &str,
    task_id: &str,
) -> Result<AgentTask, String> {
    let pool = db.0.clone();
    let session_id = session_id.to_string();
    let agent_id = agent_id.to_string();
    let task_id = task_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<AgentTask, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT id, session_id, agent_id, subject, description, status, active_form,
                    blocked_by, metadata, created_at, updated_at
             FROM agent_tasks
             WHERE id = ?1 AND session_id = ?2 AND agent_id = ?3",
            rusqlite::params![task_id.clone(), session_id, agent_id],
            parse_agent_task,
        )
        .map_err(|_| format!("task: task '{}' not found", task_id))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[allow(clippy::too_many_arguments)]
async fn update_agent_task(
    db: &DbPool,
    session_id: &str,
    agent_id: &str,
    task_id: &str,
    subject: Option<&Value>,
    description: Option<&Value>,
    status: Option<&Value>,
    active_form: Option<&Value>,
    blocked_by: Option<&Vec<String>>,
) -> Result<AgentTask, String> {
    let pool = db.0.clone();
    let session_id = session_id.to_string();
    let agent_id = agent_id.to_string();
    let task_id = task_id.to_string();
    let subject = subject
        .and_then(Value::as_str)
        .map(str::trim)
        .map(str::to_string);
    let description = description
        .and_then(Value::as_str)
        .map(str::trim)
        .map(str::to_string);
    let status = status.and_then(Value::as_str).map(str::to_string);
    let active_form = active_form
        .and_then(Value::as_str)
        .map(str::trim)
        .map(str::to_string);
    let blocked_by = blocked_by.cloned();

    tokio::task::spawn_blocking(move || -> Result<AgentTask, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let existing = conn
            .query_row(
                "SELECT id, session_id, agent_id, subject, description, status, active_form,
                        blocked_by, metadata, created_at, updated_at
                 FROM agent_tasks
                 WHERE id = ?1 AND session_id = ?2 AND agent_id = ?3",
                rusqlite::params![task_id.clone(), session_id, agent_id],
                parse_agent_task,
            )
            .map_err(|_| format!("task: task '{}' not found", task_id))?;

        let next_subject = match subject {
            Some(subject) if !subject.is_empty() => subject,
            Some(_) => return Err("task: subject must be a non-empty string".to_string()),
            None => existing.subject,
        };
        let next_description = description
            .map(|value| value.trim().to_string())
            .and_then(|value| if value.is_empty() { None } else { Some(value) })
            .or(existing.description);
        let next_status = status.unwrap_or(existing.status);
        ensure_valid_status(&next_status)?;
        let next_active_form = active_form
            .map(|value| value.trim().to_string())
            .and_then(|value| if value.is_empty() { None } else { Some(value) })
            .or(existing.active_form);
        let next_blocked_by = blocked_by.unwrap_or(existing.blocked_by);
        let blocked_by_json = serde_json::to_string(&next_blocked_by).map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();

        conn.execute(
            "UPDATE agent_tasks
             SET subject = ?1,
                 description = ?2,
                 status = ?3,
                 active_form = ?4,
                 blocked_by = ?5,
                 updated_at = ?6
             WHERE id = ?7",
            rusqlite::params![
                next_subject,
                next_description,
                next_status,
                next_active_form,
                blocked_by_json,
                now,
                task_id
            ],
        )
        .map_err(|e| e.to_string())?;

        conn.query_row(
            "SELECT id, session_id, agent_id, subject, description, status, active_form,
                    blocked_by, metadata, created_at, updated_at
             FROM agent_tasks WHERE id = ?1",
            rusqlite::params![task_id],
            parse_agent_task,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn delete_agent_task(
    db: &DbPool,
    session_id: &str,
    agent_id: &str,
    task_id: &str,
) -> Result<AgentTask, String> {
    let pool = db.0.clone();
    let session_id = session_id.to_string();
    let agent_id = agent_id.to_string();
    let task_id = task_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<AgentTask, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let deleted = conn
            .query_row(
                "SELECT id, session_id, agent_id, subject, description, status, active_form,
                        blocked_by, metadata, created_at, updated_at
                 FROM agent_tasks
                 WHERE id = ?1 AND session_id = ?2 AND agent_id = ?3",
                rusqlite::params![task_id.clone(), session_id.clone(), agent_id.clone()],
                parse_agent_task,
            )
            .map_err(|_| format!("task: task '{}' not found", task_id))?;

        conn.execute(
            "DELETE FROM agent_tasks WHERE id = ?1 AND session_id = ?2 AND agent_id = ?3",
            rusqlite::params![task_id.clone(), session_id, agent_id],
        )
        .map_err(|e| e.to_string())?;

        let mut stmt = conn
            .prepare(
                "SELECT id, blocked_by
                 FROM agent_tasks
                 WHERE session_id = ?1 AND agent_id = ?2",
            )
            .map_err(|e| e.to_string())?;
        let rows: Vec<(String, String)> = stmt
            .query_map(
                rusqlite::params![deleted.session_id, deleted.agent_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| e.to_string())?
            .filter_map(|row| row.ok())
            .collect();

        for (other_id, blocked_json) in rows {
            let mut blocked_by: Vec<String> =
                serde_json::from_str(&blocked_json).unwrap_or_default();
            let original_len = blocked_by.len();
            blocked_by.retain(|blocked| blocked != &task_id);
            if blocked_by.len() != original_len {
                let next_json = serde_json::to_string(&blocked_by).map_err(|e| e.to_string())?;
                conn.execute(
                    "UPDATE agent_tasks SET blocked_by = ?1, updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![next_json, chrono::Utc::now().to_rfc3339(), other_id],
                )
                .map_err(|e| e.to_string())?;
            }
        }

        Ok(deleted)
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn ensure_blocked_by_exists(
    db: &DbPool,
    session_id: &str,
    agent_id: &str,
    blocked_by: &[String],
) -> Result<(), String> {
    if blocked_by.is_empty() {
        return Ok(());
    }

    let pool = db.0.clone();
    let session_id = session_id.to_string();
    let agent_id = agent_id.to_string();
    let blocked_by = blocked_by.to_vec();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        for task_id in blocked_by {
            let exists: bool = conn
                .query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM agent_tasks
                        WHERE id = ?1 AND session_id = ?2 AND agent_id = ?3
                     )",
                    rusqlite::params![task_id.clone(), session_id, agent_id],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if !exists {
                return Err(format!(
                    "task: blocked_by task '{}' was not found in the current session",
                    task_id
                ));
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
