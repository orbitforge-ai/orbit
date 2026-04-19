use crate::db::cloud::CloudClientState;
use crate::db::DbPool;
use crate::models::work_item::{CreateWorkItem, UpdateWorkItem, WorkItem};
use crate::models::work_item_comment::{CommentAuthor, WorkItemComment};
use rusqlite::{params, OptionalExtension};
use ulid::Ulid;

// ── Cloud helpers ─────────────────────────────────────────────────────────────

macro_rules! cloud_upsert_work_item {
    ($cloud:expr, $item:expr) => {
        if let Some(client) = $cloud.get() {
            let w = $item.clone();
            tokio::spawn(async move {
                if let Err(e) = client.upsert_work_item(&w).await {
                    tracing::warn!("cloud upsert work_item: {}", e);
                }
            });
        }
    };
}

macro_rules! cloud_upsert_work_item_comment {
    ($cloud:expr, $comment:expr) => {
        if let Some(client) = $cloud.get() {
            let c = $comment.clone();
            tokio::spawn(async move {
                if let Err(e) = client.upsert_work_item_comment(&c).await {
                    tracing::warn!("cloud upsert work_item_comment: {}", e);
                }
            });
        }
    };
}

macro_rules! cloud_delete {
    ($cloud:expr, $table:expr, $id:expr) => {
        if let Some(client) = $cloud.get() {
            let id = $id.to_string();
            tokio::spawn(async move {
                if let Err(e) = client.delete_by_id($table, &id).await {
                    tracing::warn!("cloud delete {}: {}", $table, e);
                }
            });
        }
    };
}

// ── Row mappers ───────────────────────────────────────────────────────────────

pub(crate) fn map_work_item(row: &rusqlite::Row) -> rusqlite::Result<WorkItem> {
    let labels_json: String = row.get(11)?;
    let metadata_json: String = row.get(12)?;
    Ok(WorkItem {
        id: row.get(0)?,
        project_id: row.get(1)?,
        title: row.get(2)?,
        description: row.get(3)?,
        kind: row.get(4)?,
        status: row.get(5)?,
        priority: row.get(6)?,
        assignee_agent_id: row.get(7)?,
        created_by_agent_id: row.get(8)?,
        parent_work_item_id: row.get(9)?,
        position: row.get(10)?,
        labels: serde_json::from_str(&labels_json).unwrap_or_default(),
        metadata: serde_json::from_str(&metadata_json).unwrap_or_else(|_| serde_json::json!({})),
        blocked_reason: row.get(13)?,
        started_at: row.get(14)?,
        completed_at: row.get(15)?,
        created_at: row.get(16)?,
        updated_at: row.get(17)?,
    })
}

const WORK_ITEM_COLUMNS: &str = "id, project_id, title, description, kind, status, priority,
        assignee_agent_id, created_by_agent_id, parent_work_item_id, position,
        labels, metadata, blocked_reason, started_at, completed_at, created_at, updated_at";

fn map_work_item_comment(row: &rusqlite::Row) -> rusqlite::Result<WorkItemComment> {
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

const WORK_ITEM_COMMENT_COLUMNS: &str =
    "id, work_item_id, author_kind, author_agent_id, body, created_at, updated_at";

// ── Scope guard: reject cross-project agent writes ────────────────────────────
//
// Called from the agent tool path before any read or write. Ensures an agent
// can't list or mutate work items for projects it's not a member of. Human user
// UI calls bypass this (they're already authenticated as the workspace owner).

#[derive(Debug)]
pub enum WorkItemError {
    AgentNotInProject { project_id: String },
    NotFound(String),
    Other(String),
}

impl std::fmt::Display for WorkItemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkItemError::AgentNotInProject { project_id } => {
                write!(
                    f,
                    "agent is not a member of project '{}' (code: agent_not_in_project)",
                    project_id
                )
            }
            WorkItemError::NotFound(msg) => write!(f, "{}", msg),
            WorkItemError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl From<String> for WorkItemError {
    fn from(s: String) -> Self {
        WorkItemError::Other(s)
    }
}

pub fn assert_agent_in_project(
    conn: &rusqlite::Connection,
    agent_id: &str,
    project_id: &str,
) -> Result<(), WorkItemError> {
    crate::commands::projects::assert_agent_in_project_sync(conn, project_id, agent_id).map_err(
        |_| WorkItemError::AgentNotInProject {
            project_id: project_id.to_string(),
        },
    )
}

/// Fetch a work item row and return its project_id — used by the tool path
/// to derive the scope for a per-item operation before enforcing membership.
pub fn fetch_work_item_project(
    conn: &rusqlite::Connection,
    work_item_id: &str,
) -> Result<String, WorkItemError> {
    conn.query_row(
        "SELECT project_id FROM work_items WHERE id = ?1",
        params![work_item_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|e| WorkItemError::Other(e.to_string()))?
    .ok_or_else(|| WorkItemError::NotFound(format!("work item '{}' not found", work_item_id)))
}

// ── Work Item Commands ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_work_items(
    project_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<WorkItem>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let sql = format!(
            "SELECT {} FROM work_items WHERE project_id = ?1
             ORDER BY status, position ASC",
            WORK_ITEM_COLUMNS
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let items = stmt
            .query_map(params![project_id], map_work_item)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(items)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_work_item(id: String, db: tauri::State<'_, DbPool>) -> Result<WorkItem, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let sql = format!("SELECT {} FROM work_items WHERE id = ?1", WORK_ITEM_COLUMNS);
        conn.query_row(&sql, params![id], map_work_item)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn create_work_item(
    payload: CreateWorkItem,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<WorkItem, String> {
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let item = tokio::task::spawn_blocking(move || -> Result<WorkItem, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let id = Ulid::new().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let kind = payload.kind.unwrap_or_else(|| "task".to_string());
        let status = payload.status.unwrap_or_else(|| "backlog".to_string());
        let priority = payload.priority.unwrap_or(0);
        // When position is not supplied, append to the end of the target column
        // by using (max + 1024.0) — gap-based float ordering.
        let position = match payload.position {
            Some(p) => p,
            None => {
                let max: Option<f64> = conn
                    .query_row(
                        "SELECT MAX(position) FROM work_items WHERE project_id = ?1 AND status = ?2",
                        params![payload.project_id, status],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?
                    .flatten();
                max.unwrap_or(0.0) + 1024.0
            }
        };
        let labels_json = serde_json::to_string(&payload.labels.unwrap_or_default())
            .map_err(|e| e.to_string())?;
        let metadata_json = serde_json::to_string(
            &payload.metadata.unwrap_or_else(|| serde_json::json!({})),
        )
        .map_err(|e| e.to_string())?;

        if status == "blocked" {
            return Err("work_item: cannot create a card with status='blocked' without a reason; create first then block".into());
        }

        conn.execute(
            "INSERT INTO work_items (
                id, project_id, title, description, kind, status, priority,
                assignee_agent_id, created_by_agent_id, parent_work_item_id, position,
                labels, metadata, blocked_reason, started_at, completed_at, created_at, updated_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,NULL,NULL,NULL,?14,?14)",
            params![
                id,
                payload.project_id,
                payload.title,
                payload.description,
                kind,
                status,
                priority,
                payload.assignee_agent_id,
                payload.created_by_agent_id,
                payload.parent_work_item_id,
                position,
                labels_json,
                metadata_json,
                now,
            ],
        )
        .map_err(|e| e.to_string())?;

        let sql = format!("SELECT {} FROM work_items WHERE id = ?1", WORK_ITEM_COLUMNS);
        conn.query_row(&sql, params![id], map_work_item)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_upsert_work_item!(cloud, item);
    Ok(item)
}

#[tauri::command]
pub async fn update_work_item(
    id: String,
    payload: UpdateWorkItem,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<WorkItem, String> {
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let item = tokio::task::spawn_blocking(move || -> Result<WorkItem, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();

        if let Some(title) = &payload.title {
            if title.trim().is_empty() {
                return Err("work_item: title must be non-empty".into());
            }
            conn.execute(
                "UPDATE work_items SET title = ?1, updated_at = ?2 WHERE id = ?3",
                params![title, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(description) = &payload.description {
            conn.execute(
                "UPDATE work_items SET description = ?1, updated_at = ?2 WHERE id = ?3",
                params![description, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(kind) = &payload.kind {
            conn.execute(
                "UPDATE work_items SET kind = ?1, updated_at = ?2 WHERE id = ?3",
                params![kind, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(priority) = payload.priority {
            conn.execute(
                "UPDATE work_items SET priority = ?1, updated_at = ?2 WHERE id = ?3",
                params![priority, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(labels) = &payload.labels {
            let labels_json = serde_json::to_string(labels).map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE work_items SET labels = ?1, updated_at = ?2 WHERE id = ?3",
                params![labels_json, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(metadata) = &payload.metadata {
            let metadata_json = serde_json::to_string(metadata).map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE work_items SET metadata = ?1, updated_at = ?2 WHERE id = ?3",
                params![metadata_json, now, id],
            )
            .map_err(|e| e.to_string())?;
        }

        let sql = format!("SELECT {} FROM work_items WHERE id = ?1", WORK_ITEM_COLUMNS);
        conn.query_row(&sql, params![id], map_work_item)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_upsert_work_item!(cloud, item);
    Ok(item)
}

#[tauri::command]
pub async fn delete_work_item(
    id: String,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let id_clone = id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM work_items WHERE id = ?1", params![id_clone])
            .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_delete!(cloud, "work_items", id);
    Ok(())
}

#[tauri::command]
pub async fn claim_work_item(
    id: String,
    agent_id: String,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<WorkItem, String> {
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let item = tokio::task::spawn_blocking(move || -> Result<WorkItem, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE work_items
                SET assignee_agent_id = ?1,
                    status = 'in_progress',
                    started_at = COALESCE(started_at, ?2),
                    updated_at = ?2
              WHERE id = ?3",
            params![agent_id, now, id],
        )
        .map_err(|e| e.to_string())?;

        let sql = format!("SELECT {} FROM work_items WHERE id = ?1", WORK_ITEM_COLUMNS);
        conn.query_row(&sql, params![id], map_work_item)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_upsert_work_item!(cloud, item);
    Ok(item)
}

#[tauri::command]
pub async fn move_work_item(
    id: String,
    status: String,
    position: Option<f64>,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<WorkItem, String> {
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let item = tokio::task::spawn_blocking(move || -> Result<WorkItem, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();

        let current_status: String = conn
            .query_row(
                "SELECT status FROM work_items WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        let project_id: String = conn
            .query_row(
                "SELECT project_id FROM work_items WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;

        if status == "blocked" {
            let reason_ok: bool = conn
                .query_row(
                    "SELECT blocked_reason IS NOT NULL AND length(blocked_reason) > 0
                       FROM work_items WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
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
                let max: Option<f64> = conn
                    .query_row(
                        "SELECT MAX(position) FROM work_items WHERE project_id = ?1 AND status = ?2",
                        params![project_id, status],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?
                    .flatten();
                max.unwrap_or(0.0) + 1024.0
            }
        };

        // Stamp transition markers
        let started_at_expr = if current_status != "in_progress" && status == "in_progress" {
            "COALESCE(started_at, ?4)"
        } else {
            "started_at"
        };
        let completed_at_expr = if status == "done" {
            "?4"
        } else if status == "cancelled" {
            "?4"
        } else {
            "completed_at"
        };

        let sql = format!(
            "UPDATE work_items
                SET status = ?1,
                    position = ?2,
                    started_at = {},
                    completed_at = {},
                    updated_at = ?4
              WHERE id = ?3",
            started_at_expr, completed_at_expr
        );
        conn.execute(&sql, params![status, position, id, now])
            .map_err(|e| e.to_string())?;

        let sql = format!("SELECT {} FROM work_items WHERE id = ?1", WORK_ITEM_COLUMNS);
        conn.query_row(&sql, params![id], map_work_item)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_upsert_work_item!(cloud, item);
    Ok(item)
}

#[tauri::command]
pub async fn reorder_work_items(
    project_id: String,
    status: String,
    ordered_ids: Vec<String>,
    db: tauri::State<'_, DbPool>,
) -> Result<(), String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        for (idx, item_id) in ordered_ids.iter().enumerate() {
            let pos = ((idx + 1) as f64) * 1024.0;
            tx.execute(
                "UPDATE work_items
                    SET position = ?1, updated_at = ?2
                  WHERE id = ?3 AND project_id = ?4 AND status = ?5",
                params![pos, now, item_id, project_id, status],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn block_work_item(
    id: String,
    reason: String,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<WorkItem, String> {
    if reason.trim().is_empty() {
        return Err("work_item: blocked_reason must be non-empty".into());
    }
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let item = tokio::task::spawn_blocking(move || -> Result<WorkItem, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE work_items
                SET status = 'blocked', blocked_reason = ?1, updated_at = ?2
              WHERE id = ?3",
            params![reason, now, id],
        )
        .map_err(|e| e.to_string())?;
        let sql = format!("SELECT {} FROM work_items WHERE id = ?1", WORK_ITEM_COLUMNS);
        conn.query_row(&sql, params![id], map_work_item)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_upsert_work_item!(cloud, item);
    Ok(item)
}

#[tauri::command]
pub async fn complete_work_item(
    id: String,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<WorkItem, String> {
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let item = tokio::task::spawn_blocking(move || -> Result<WorkItem, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE work_items
                SET status = 'done',
                    completed_at = ?1,
                    blocked_reason = NULL,
                    updated_at = ?1
              WHERE id = ?2",
            params![now, id],
        )
        .map_err(|e| e.to_string())?;
        let sql = format!("SELECT {} FROM work_items WHERE id = ?1", WORK_ITEM_COLUMNS);
        conn.query_row(&sql, params![id], map_work_item)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_upsert_work_item!(cloud, item);
    Ok(item)
}

// ── Work Item Comment Commands ────────────────────────────────────────────────

#[tauri::command]
pub async fn list_work_item_comments(
    work_item_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<WorkItemComment>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let sql = format!(
            "SELECT {} FROM work_item_comments WHERE work_item_id = ?1 ORDER BY created_at ASC",
            WORK_ITEM_COMMENT_COLUMNS
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let comments = stmt
            .query_map(params![work_item_id], map_work_item_comment)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(comments)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn create_work_item_comment(
    work_item_id: String,
    body: String,
    author: CommentAuthor,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<WorkItemComment, String> {
    if body.trim().is_empty() {
        return Err("work_item_comment: body must be non-empty".into());
    }
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let comment = tokio::task::spawn_blocking(move || -> Result<WorkItemComment, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
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
            params![id, work_item_id, author_kind, author_agent_id, body, now],
        )
        .map_err(|e| e.to_string())?;
        let sql = format!(
            "SELECT {} FROM work_item_comments WHERE id = ?1",
            WORK_ITEM_COMMENT_COLUMNS
        );
        conn.query_row(&sql, params![id], map_work_item_comment)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_upsert_work_item_comment!(cloud, comment);
    Ok(comment)
}

#[tauri::command]
pub async fn update_work_item_comment(
    id: String,
    body: String,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<WorkItemComment, String> {
    if body.trim().is_empty() {
        return Err("work_item_comment: body must be non-empty".into());
    }
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let comment = tokio::task::spawn_blocking(move || -> Result<WorkItemComment, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE work_item_comments SET body = ?1, updated_at = ?2 WHERE id = ?3",
            params![body, now, id],
        )
        .map_err(|e| e.to_string())?;
        let sql = format!(
            "SELECT {} FROM work_item_comments WHERE id = ?1",
            WORK_ITEM_COMMENT_COLUMNS
        );
        conn.query_row(&sql, params![id], map_work_item_comment)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_upsert_work_item_comment!(cloud, comment);
    Ok(comment)
}

#[tauri::command]
pub async fn delete_work_item_comment(
    id: String,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let id_clone = id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM work_item_comments WHERE id = ?1",
            params![id_clone],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_delete!(cloud, "work_item_comments", id);
    Ok(())
}
