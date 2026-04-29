use crate::commands::project_board_columns::{
    list_project_board_columns_sync, resolve_board_column_sync,
};
use crate::commands::work_item_events::{event_kind, insert_event, Actor};
use crate::db::cloud::CloudClientState;
use crate::db::DbPool;
use crate::models::project_board_column::ProjectBoardColumn;
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

const WORK_ITEM_COLUMNS: &str =
    "id, project_id, board_id, title, description, kind, column_id, status, priority,
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

fn resolve_target_column(
    conn: &rusqlite::Connection,
    project_id: &str,
    board_id: Option<&str>,
    column_id: Option<&str>,
    status: Option<&str>,
) -> Result<ProjectBoardColumn, String> {
    resolve_board_column_sync(conn, project_id, board_id, column_id, status)
}

fn resolve_create_status(column: &ProjectBoardColumn, requested_status: Option<&str>) -> String {
    column
        .role
        .clone()
        .or_else(|| requested_status.map(str::to_string))
        .unwrap_or_else(|| "backlog".to_string())
}

fn resolve_move_status(column: &ProjectBoardColumn, current_status: &str) -> String {
    column
        .role
        .clone()
        .unwrap_or_else(|| current_status.to_string())
}

fn resolve_next_column(
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

pub async fn create_work_item_with_db(
    db: &DbPool,
    payload: CreateWorkItem,
) -> Result<WorkItem, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || -> Result<WorkItem, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let id = Ulid::new().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let kind = payload.kind.unwrap_or_else(|| "task".to_string());
        let column = resolve_target_column(
            &conn,
            &payload.project_id,
            payload.board_id.as_deref(),
            payload.column_id.as_deref(),
            payload.status.as_deref(),
        )?;
        let column_id = column.id.clone();
        let board_id = column.board_id.clone();
        let status = resolve_create_status(&column, payload.status.as_deref());
        let priority = payload.priority.unwrap_or(0);
        let position = match payload.position {
            Some(p) => p,
            None => {
                let max: Option<f64> = conn
                    .query_row(
                        "SELECT MAX(position) FROM work_items WHERE project_id = ?1 AND column_id = ?2",
                        params![payload.project_id, column_id],
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
                id, project_id, board_id, title, description, kind, column_id, status, priority,
                assignee_agent_id, created_by_agent_id, parent_work_item_id, position,
                labels, metadata, blocked_reason, started_at, completed_at, created_at, updated_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,NULL,NULL,NULL,?16,?16)",
            params![
                id,
                payload.project_id,
                board_id,
                payload.title,
                payload.description,
                kind,
                column_id,
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

        insert_event(
            &conn,
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

        let sql = format!("SELECT {} FROM work_items WHERE id = ?1", WORK_ITEM_COLUMNS);
        conn.query_row(&sql, params![id], map_work_item)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn list_work_items_with_db(
    db: &DbPool,
    project_id: String,
    board_id: Option<String>,
) -> Result<Vec<WorkItem>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        match board_id {
            Some(board_id) => {
                let sql = format!(
                    "SELECT {} FROM work_items WHERE project_id = ?1 AND board_id = ?2
                     ORDER BY COALESCE(column_id, status), position ASC",
                    WORK_ITEM_COLUMNS
                );
                let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
                let items = stmt
                    .query_map(params![project_id, board_id], map_work_item)
                    .map_err(|e| e.to_string())?
                    .filter_map(|r| r.ok())
                    .collect();
                Ok(items)
            }
            None => {
                let sql = format!(
                    "SELECT {} FROM work_items WHERE project_id = ?1
                     ORDER BY COALESCE(column_id, status), position ASC",
                    WORK_ITEM_COLUMNS
                );
                let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
                let items = stmt
                    .query_map(params![project_id], map_work_item)
                    .map_err(|e| e.to_string())?
                    .filter_map(|r| r.ok())
                    .collect();
                Ok(items)
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn get_work_item_with_db(db: &DbPool, id: String) -> Result<WorkItem, String> {
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

pub async fn update_work_item_with_db(
    db: &DbPool,
    id: String,
    payload: UpdateWorkItem,
) -> Result<WorkItem, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || -> Result<WorkItem, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        let sql_before = format!("SELECT {} FROM work_items WHERE id = ?1", WORK_ITEM_COLUMNS);
        let before: WorkItem = conn
            .query_row(&sql_before, params![id], map_work_item)
            .map_err(|e| e.to_string())?;
        let project_id = before.project_id.clone();

        if let Some(title) = &payload.title {
            if title.trim().is_empty() {
                return Err("work_item: title must be non-empty".into());
            }
            if title != &before.title {
                conn.execute(
                    "UPDATE work_items SET title = ?1, updated_at = ?2 WHERE id = ?3",
                    params![title, now, id],
                )
                .map_err(|e| e.to_string())?;
                insert_event(
                    &conn,
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
                    params![description, now, id],
                )
                .map_err(|e| e.to_string())?;
                insert_event(
                    &conn,
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
                    params![kind, now, id],
                )
                .map_err(|e| e.to_string())?;
                insert_event(
                    &conn,
                    &id,
                    Actor::System,
                    event_kind::KIND_CHANGED,
                    serde_json::json!({ "from": before.kind, "to": kind }),
                )?;
            }
        }
        if let Some(column_id) = payload.column_id.as_deref() {
            let resolved_column =
                resolve_target_column(&conn, &project_id, None, Some(column_id), None)?;
            if Some(resolved_column.id.as_str()) != before.column_id.as_deref() {
                conn.execute(
                    "UPDATE work_items
                        SET column_id = ?1, updated_at = ?2
                      WHERE id = ?3",
                    params![resolved_column.id, now, id],
                )
                .map_err(|e| e.to_string())?;
                insert_event(
                    &conn,
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
                    params![priority, now, id],
                )
                .map_err(|e| e.to_string())?;
                insert_event(
                    &conn,
                    &id,
                    Actor::System,
                    event_kind::PRIORITY_CHANGED,
                    serde_json::json!({ "from": before.priority, "to": priority }),
                )?;
            }
        }
        if let Some(labels) = &payload.labels {
            if labels != &before.labels {
                let labels_json = serde_json::to_string(labels).map_err(|e| e.to_string())?;
                conn.execute(
                    "UPDATE work_items SET labels = ?1, updated_at = ?2 WHERE id = ?3",
                    params![labels_json, now, id],
                )
                .map_err(|e| e.to_string())?;
                insert_event(
                    &conn,
                    &id,
                    Actor::System,
                    event_kind::LABELS_CHANGED,
                    serde_json::json!({ "from": before.labels, "to": labels }),
                )?;
            }
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
    .map_err(|e| e.to_string())?
}

pub async fn delete_work_item_with_db(db: &DbPool, id: String) -> Result<(), String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM work_items WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn claim_work_item_with_db(
    db: &DbPool,
    id: String,
    agent_id: String,
) -> Result<WorkItem, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || -> Result<WorkItem, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        let sql_before = format!("SELECT {} FROM work_items WHERE id = ?1", WORK_ITEM_COLUMNS);
        let before: WorkItem = conn
            .query_row(&sql_before, params![id], map_work_item)
            .map_err(|e| e.to_string())?;
        let project_id = before.project_id.clone();
        let board_id = before.board_id.clone();
        let column = resolve_target_column(
            &conn,
            &project_id,
            board_id.as_deref(),
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
            params![agent_id, column_id, status, now, id],
        )
        .map_err(|e| e.to_string())?;

        if before.assignee_agent_id.as_deref() != Some(agent_id.as_str()) {
            insert_event(
                &conn,
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
                &conn,
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

        let sql = format!("SELECT {} FROM work_items WHERE id = ?1", WORK_ITEM_COLUMNS);
        conn.query_row(&sql, params![id], map_work_item)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn move_work_item_with_db(
    db: &DbPool,
    id: String,
    column_id: Option<String>,
    position: Option<f64>,
) -> Result<WorkItem, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || -> Result<WorkItem, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();

        let (current_status, current_column_id, project_id, current_board_id): (
            String,
            Option<String>,
            String,
            Option<String>,
        ) = conn
            .query_row(
                "SELECT status, column_id, project_id, board_id FROM work_items WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|e| e.to_string())?;
        let column = match column_id.as_deref() {
            Some(column_id) => resolve_target_column(
                &conn,
                &project_id,
                current_board_id.as_deref(),
                Some(column_id),
                None,
            )?,
            None => resolve_next_column(
                &conn,
                &project_id,
                current_board_id.as_deref(),
                current_column_id.as_deref(),
            )?,
        };
        let column_id = column.id.clone();
        let status = resolve_move_status(&column, &current_status);

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
                if current_status == status && current_column_id.as_deref() == Some(column_id.as_str()) {
                    let current_position: f64 = conn
                        .query_row(
                            "SELECT position FROM work_items WHERE id = ?1",
                            params![id],
                            |row| row.get(0),
                        )
                        .map_err(|e| e.to_string())?;
                    current_position
                } else {
                let max: Option<f64> = conn
                    .query_row(
                        "SELECT MAX(position) FROM work_items WHERE project_id = ?1 AND column_id = ?2",
                        params![project_id, column_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?
                    .flatten();
                max.unwrap_or(0.0) + 1024.0
                }
            }
        };

        let started_at_expr = if current_status != "in_progress" && status == "in_progress" {
            "COALESCE(started_at, ?4)"
        } else {
            "started_at"
        };
        let completed_at_expr = if status == "done" || status == "cancelled" {
            "?4"
        } else {
            "completed_at"
        };
        let blocked_reason_expr = if current_status == "blocked" && status != "blocked" {
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
        conn.execute(&sql, params![column_id, status, position, id, now])
            .map_err(|e| e.to_string())?;

        if current_column_id.as_deref() != Some(column_id.as_str()) {
            insert_event(
                &conn,
                &id,
                Actor::System,
                event_kind::COLUMN_CHANGED,
                serde_json::json!({
                    "fromColumnId": current_column_id,
                    "fromStatus": current_status,
                    "toColumnId": column_id,
                    "toColumnName": column.name,
                    "toStatus": status,
                }),
            )?;
        }
        if status == "done" && current_status != "done" {
            insert_event(
                &conn,
                &id,
                Actor::System,
                event_kind::COMPLETED,
                serde_json::json!({ "via": "move" }),
            )?;
        }
        if current_status == "blocked" && status != "blocked" {
            insert_event(
                &conn,
                &id,
                Actor::System,
                event_kind::UNBLOCKED,
                serde_json::json!({ "via": "move" }),
            )?;
        }

        let sql = format!("SELECT {} FROM work_items WHERE id = ?1", WORK_ITEM_COLUMNS);
        conn.query_row(&sql, params![id], map_work_item)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn block_work_item_with_db(
    db: &DbPool,
    id: String,
    reason: String,
) -> Result<WorkItem, String> {
    if reason.trim().is_empty() {
        return Err("work_item: blocked_reason must be non-empty".into());
    }
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || -> Result<WorkItem, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        let (project_id, board_id): (String, Option<String>) = conn
            .query_row(
                "SELECT project_id, board_id FROM work_items WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        let column = resolve_target_column(
            &conn,
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
            params![column_id, status, reason, now, id],
        )
        .map_err(|e| e.to_string())?;
        insert_event(
            &conn,
            &id,
            Actor::System,
            event_kind::BLOCKED,
            serde_json::json!({ "reason": reason }),
        )?;
        let sql = format!("SELECT {} FROM work_items WHERE id = ?1", WORK_ITEM_COLUMNS);
        conn.query_row(&sql, params![id], map_work_item)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn complete_work_item_with_db(db: &DbPool, id: String) -> Result<WorkItem, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || -> Result<WorkItem, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        let (project_id, board_id): (String, Option<String>) = conn
            .query_row(
                "SELECT project_id, board_id FROM work_items WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        let column =
            resolve_target_column(&conn, &project_id, board_id.as_deref(), None, Some("done"))?;
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
            params![column_id, status, now, id],
        )
        .map_err(|e| e.to_string())?;
        insert_event(
            &conn,
            &id,
            Actor::System,
            event_kind::COMPLETED,
            serde_json::json!({ "via": "complete" }),
        )?;
        let sql = format!("SELECT {} FROM work_items WHERE id = ?1", WORK_ITEM_COLUMNS);
        conn.query_row(&sql, params![id], map_work_item)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn unblock_work_item_with_db(
    db: &DbPool,
    id: String,
    status: String,
) -> Result<WorkItem, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || -> Result<WorkItem, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        let (project_id, board_id): (String, Option<String>) = conn
            .query_row(
                "SELECT project_id, board_id FROM work_items WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        let column =
            resolve_target_column(&conn, &project_id, board_id.as_deref(), None, Some(&status))?;
        let column_id = column.id;
        let resolved_status = column.role.unwrap_or(status);
        conn.execute(
            "UPDATE work_items
                SET column_id = ?1,
                    status = ?2,
                    blocked_reason = NULL,
                    updated_at = ?3
              WHERE id = ?4",
            params![column_id, resolved_status, now, id],
        )
        .map_err(|e| e.to_string())?;
        insert_event(
            &conn,
            &id,
            Actor::System,
            event_kind::UNBLOCKED,
            serde_json::json!({ "toStatus": resolved_status }),
        )?;
        let sql = format!("SELECT {} FROM work_items WHERE id = ?1", WORK_ITEM_COLUMNS);
        conn.query_row(&sql, params![id], map_work_item)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn list_work_item_comments_with_db(
    db: &DbPool,
    work_item_id: String,
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

pub async fn create_work_item_comment_with_db(
    db: &DbPool,
    work_item_id: String,
    body: String,
    author: CommentAuthor,
) -> Result<WorkItemComment, String> {
    if body.trim().is_empty() {
        return Err("work_item_comment: body must be non-empty".into());
    }
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || -> Result<WorkItemComment, String> {
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
        let actor = match author_kind {
            "agent" => match author_agent_id.as_deref() {
                Some(aid) => Actor::Agent { agent_id: aid },
                None => Actor::System,
            },
            "user" => Actor::User,
            _ => Actor::System,
        };
        insert_event(
            &conn,
            &work_item_id,
            actor,
            event_kind::COMMENT_ADDED,
            serde_json::json!({ "commentId": id }),
        )?;
        let sql = format!(
            "SELECT {} FROM work_item_comments WHERE id = ?1",
            WORK_ITEM_COMMENT_COLUMNS
        );
        conn.query_row(&sql, params![id], map_work_item_comment)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

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
    board_id: Option<String>,
    app: tauri::State<'_, crate::app_context::AppContext>,
) -> Result<Vec<WorkItem>, String> {
    app.repos.work_items().list(&project_id, board_id).await
}

#[tauri::command]
pub async fn get_work_item(
    id: String,
    app: tauri::State<'_, crate::app_context::AppContext>,
) -> Result<WorkItem, String> {
    app.repos.work_items().get(&id).await
}

#[tauri::command]
pub async fn create_work_item(
    payload: CreateWorkItem,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<WorkItem, String> {
    let cloud = cloud.inner().clone();
    let item = create_work_item_with_db(db.inner(), payload).await?;

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
    let item = update_work_item_with_db(db.inner(), id, payload).await?;

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
    delete_work_item_with_db(db.inner(), id.clone()).await?;

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
    let item = claim_work_item_with_db(db.inner(), id, agent_id).await?;

    cloud_upsert_work_item!(cloud, item);
    Ok(item)
}

#[tauri::command]
pub async fn move_work_item(
    id: String,
    column_id: Option<String>,
    position: Option<f64>,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<WorkItem, String> {
    let cloud = cloud.inner().clone();
    let item = move_work_item_with_db(db.inner(), id, column_id, position).await?;

    cloud_upsert_work_item!(cloud, item);
    Ok(item)
}

#[tauri::command]
pub async fn reorder_work_items(
    project_id: String,
    board_id: Option<String>,
    status: Option<String>,
    column_id: Option<String>,
    ordered_ids: Vec<String>,
    db: tauri::State<'_, DbPool>,
) -> Result<(), String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        let resolved_column_id = resolve_target_column(
            &conn,
            &project_id,
            board_id.as_deref(),
            column_id.as_deref(),
            status.as_deref(),
        )?
        .id;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        for (idx, item_id) in ordered_ids.iter().enumerate() {
            let pos = ((idx + 1) as f64) * 1024.0;
            tx.execute(
                "UPDATE work_items
                    SET position = ?1, updated_at = ?2
                  WHERE id = ?3 AND project_id = ?4 AND column_id = ?5",
                params![pos, now, item_id, project_id, resolved_column_id],
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
    let cloud = cloud.inner().clone();
    let item = block_work_item_with_db(db.inner(), id, reason).await?;

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
    let item = complete_work_item_with_db(db.inner(), id).await?;

    cloud_upsert_work_item!(cloud, item);
    Ok(item)
}

// ── Work Item Comment Commands ────────────────────────────────────────────────

#[tauri::command]
pub async fn list_work_item_comments(
    work_item_id: String,
    app: tauri::State<'_, crate::app_context::AppContext>,
) -> Result<Vec<WorkItemComment>, String> {
    app.repos.work_items().list_comments(&work_item_id).await
}

#[tauri::command]
pub async fn create_work_item_comment(
    work_item_id: String,
    body: String,
    author: CommentAuthor,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<WorkItemComment, String> {
    let cloud = cloud.inner().clone();
    let comment = create_work_item_comment_with_db(db.inner(), work_item_id, body, author).await?;

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
        let comment: WorkItemComment = conn
            .query_row(&sql, params![id], map_work_item_comment)
            .map_err(|e| e.to_string())?;
        insert_event(
            &conn,
            &comment.work_item_id,
            Actor::System,
            event_kind::COMMENT_EDITED,
            serde_json::json!({ "commentId": comment.id }),
        )?;
        Ok(comment)
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
        let work_item_id: Option<String> = conn
            .query_row(
                "SELECT work_item_id FROM work_item_comments WHERE id = ?1",
                params![id_clone],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM work_item_comments WHERE id = ?1",
            params![id_clone],
        )
        .map_err(|e| e.to_string())?;
        if let Some(wid) = work_item_id {
            insert_event(
                &conn,
                &wid,
                Actor::System,
                event_kind::COMMENT_DELETED,
                serde_json::json!({ "commentId": id_clone }),
            )?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_delete!(cloud, "work_item_comments", id);
    Ok(())
}

mod http {
    use tauri::Manager;

    use super::*;
    use crate::db::cloud::CloudClientState;
    use crate::db::DbPool;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ListArgs { project_id: String, #[serde(default)] board_id: Option<String> }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct IdArgs { id: String }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CreateArgs { payload: CreateWorkItem }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UpdateArgs { id: String, payload: UpdateWorkItem }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ClaimArgs { id: String, agent_id: String }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct MoveArgs { id: String, #[serde(default)] column_id: Option<String>, #[serde(default)] position: Option<f64> }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ReorderArgs {
        project_id: String,
        #[serde(default)] board_id: Option<String>,
        #[serde(default)] status: Option<String>,
        #[serde(default)] column_id: Option<String>,
        ordered_ids: Vec<String>,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct BlockArgs { id: String, reason: String }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct WorkItemIdArgs { work_item_id: String }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CreateCommentArgs { work_item_id: String, body: String, author: CommentAuthor }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UpdateCommentArgs { id: String, body: String }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("list_work_items", |ctx, args| async move {
            let a: ListArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx.repos.work_items().list(&a.project_id, a.board_id).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("get_work_item", |ctx, args| async move {
            let a: IdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx.repos.work_items().get(&a.id).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("create_work_item", |ctx, args| async move {
            let app = ctx.app()?;
            let a: CreateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = create_work_item(a.payload, app.state::<DbPool>(), app.state::<CloudClientState>()).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("update_work_item", |ctx, args| async move {
            let app = ctx.app()?;
            let a: UpdateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = update_work_item(a.id, a.payload, app.state::<DbPool>(), app.state::<CloudClientState>()).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("delete_work_item", |ctx, args| async move {
            let app = ctx.app()?;
            let a: IdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            delete_work_item(a.id, app.state::<DbPool>(), app.state::<CloudClientState>()).await?;
            Ok(serde_json::Value::Null)
        });
        reg.register("claim_work_item", |ctx, args| async move {
            let app = ctx.app()?;
            let a: ClaimArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = claim_work_item(a.id, a.agent_id, app.state::<DbPool>(), app.state::<CloudClientState>()).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("move_work_item", |ctx, args| async move {
            let app = ctx.app()?;
            let a: MoveArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = move_work_item(a.id, a.column_id, a.position, app.state::<DbPool>(), app.state::<CloudClientState>()).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("reorder_work_items", |ctx, args| async move {
            let app = ctx.app()?;
            let a: ReorderArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            reorder_work_items(a.project_id, a.board_id, a.status, a.column_id, a.ordered_ids, app.state::<DbPool>()).await?;
            Ok(serde_json::Value::Null)
        });
        reg.register("block_work_item", |ctx, args| async move {
            let app = ctx.app()?;
            let a: BlockArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = block_work_item(a.id, a.reason, app.state::<DbPool>(), app.state::<CloudClientState>()).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("complete_work_item", |ctx, args| async move {
            let app = ctx.app()?;
            let a: IdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = complete_work_item(a.id, app.state::<DbPool>(), app.state::<CloudClientState>()).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("list_work_item_comments", |ctx, args| async move {
            let a: WorkItemIdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx.repos.work_items().list_comments(&a.work_item_id).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("create_work_item_comment", |ctx, args| async move {
            let app = ctx.app()?;
            let a: CreateCommentArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = create_work_item_comment(a.work_item_id, a.body, a.author, app.state::<DbPool>(), app.state::<CloudClientState>()).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("update_work_item_comment", |ctx, args| async move {
            let app = ctx.app()?;
            let a: UpdateCommentArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = update_work_item_comment(a.id, a.body, app.state::<DbPool>(), app.state::<CloudClientState>()).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("delete_work_item_comment", |ctx, args| async move {
            let app = ctx.app()?;
            let a: IdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            delete_work_item_comment(a.id, app.state::<DbPool>(), app.state::<CloudClientState>()).await?;
            Ok(serde_json::Value::Null)
        });
    }
}

pub use http::register as register_http;
