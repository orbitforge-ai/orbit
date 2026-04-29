use crate::app_context::AppContext;
use crate::commands::project_boards::{get_default_board_sync, resolve_board_sync};
use crate::models::project_board_column::{
    CreateProjectBoardColumn, DeleteProjectBoardColumn, ProjectBoardColumn,
    ReorderProjectBoardColumns, UpdateProjectBoardColumn,
};
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use ulid::Ulid;

pub const LEGACY_BOARD_ROLES: &[&str] = &[
    "backlog",
    "todo",
    "in_progress",
    "blocked",
    "review",
    "done",
    "cancelled",
];

const COLUMN_SELECT: &str =
    "id, project_id, board_id, name, role, is_default, position, created_at, updated_at";

macro_rules! cloud_upsert_board_column {
    ($cloud:expr, $column:expr) => {
        if let Some(client) = $cloud.get() {
            let column = $column.clone();
            tokio::spawn(async move {
                if let Err(e) = client.upsert_project_board_column(&column).await {
                    tracing::warn!("cloud upsert project_board_column: {}", e);
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

#[derive(Debug, Clone, Copy)]
pub struct BoardPresetColumn {
    pub name: &'static str,
    pub role: Option<&'static str>,
    pub is_default: bool,
}

pub fn board_preset_columns(preset_id: Option<&str>) -> Vec<BoardPresetColumn> {
    match preset_id.unwrap_or("starter") {
        "lean" => vec![
            BoardPresetColumn {
                name: "Inbox",
                role: Some("backlog"),
                is_default: true,
            },
            BoardPresetColumn {
                name: "In Progress",
                role: Some("in_progress"),
                is_default: false,
            },
            BoardPresetColumn {
                name: "Review",
                role: Some("review"),
                is_default: false,
            },
            BoardPresetColumn {
                name: "Done",
                role: Some("done"),
                is_default: false,
            },
        ],
        _ => vec![
            BoardPresetColumn {
                name: "Backlog",
                role: Some("backlog"),
                is_default: true,
            },
            BoardPresetColumn {
                name: "Todo",
                role: Some("todo"),
                is_default: false,
            },
            BoardPresetColumn {
                name: "In Progress",
                role: Some("in_progress"),
                is_default: false,
            },
            BoardPresetColumn {
                name: "Blocked",
                role: Some("blocked"),
                is_default: false,
            },
            BoardPresetColumn {
                name: "Review",
                role: Some("review"),
                is_default: false,
            },
            BoardPresetColumn {
                name: "Done",
                role: Some("done"),
                is_default: false,
            },
            BoardPresetColumn {
                name: "Cancelled",
                role: Some("cancelled"),
                is_default: false,
            },
        ],
    }
}

pub(crate) fn map_project_board_column(
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

pub fn validate_board_role(role: Option<&str>) -> Result<(), String> {
    match role {
        Some(role) if !LEGACY_BOARD_ROLES.contains(&role) => {
            Err(format!("invalid board role '{}'", role))
        }
        _ => Ok(()),
    }
}

pub fn is_terminal_role(role: Option<&str>) -> bool {
    matches!(role, Some("done" | "cancelled"))
}

fn require_project_column_sync(
    conn: &rusqlite::Connection,
    project_id: &str,
    column_id: &str,
) -> Result<ProjectBoardColumn, String> {
    let column = get_column_by_id_sync(conn, column_id)?
        .ok_or_else(|| format!("board column '{}' not found", column_id))?;
    if column.project_id != project_id {
        return Err(format!(
            "board column '{}' does not belong to project '{}'",
            column_id, project_id
        ));
    }
    Ok(column)
}

pub fn ensure_project_board_columns(
    conn: &rusqlite::Connection,
    project_id: &str,
    created_at: &str,
    preset_id: Option<&str>,
) -> Result<(), String> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM project_board_columns WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if count > 0 {
        return Ok(());
    }

    let board = match get_default_board_sync(conn, project_id).map_err(|e| e.to_string())? {
        Some(board) => board,
        None => {
            let board_id = Ulid::new().to_string();
            conn.execute(
                "INSERT INTO project_boards (id, project_id, name, prefix, position, is_default, created_at, updated_at, tenant_id)
                 VALUES (?1, ?2, 'Default', 'MAIN', 1024.0, 1, ?3, ?3, COALESCE((SELECT tenant_id FROM projects WHERE id = ?2), 'local'))",
                params![board_id, project_id, created_at],
            )
            .map_err(|e| e.to_string())?;
            get_default_board_sync(conn, project_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "failed to create default board".to_string())?
        }
    };

    for (idx, column) in board_preset_columns(preset_id).into_iter().enumerate() {
        conn.execute(
            "INSERT INTO project_board_columns (
                id, project_id, board_id, name, status, role, is_default, position, created_at, updated_at, tenant_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, COALESCE((SELECT tenant_id FROM projects WHERE id = ?2), 'local'))",
            params![
                format!(
                    "col_{}_{}",
                    project_id,
                    column
                        .role
                        .unwrap_or_else(|| if idx == 0 { "default" } else { "column" })
                ),
                project_id,
                board.id,
                column.name,
                column.role.unwrap_or("backlog"),
                column.role,
                column.is_default,
                ((idx + 1) as f64) * 1024.0,
                created_at,
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn list_project_board_columns_sync(
    conn: &rusqlite::Connection,
    project_id: &str,
    board_id: Option<&str>,
) -> Result<Vec<ProjectBoardColumn>, String> {
    let effective_board_id = match board_id {
        Some(id) => Some(id.to_string()),
        None => get_default_board_sync(conn, project_id)?.map(|b| b.id),
    };
    let (sql, params_vec): (String, Vec<&dyn rusqlite::ToSql>) = match &effective_board_id {
        Some(board) => (
            format!(
                "SELECT {} FROM project_board_columns WHERE project_id = ?1 AND board_id = ?2 ORDER BY position ASC, created_at ASC",
                COLUMN_SELECT
            ),
            vec![&project_id as &dyn rusqlite::ToSql, board as &dyn rusqlite::ToSql],
        ),
        None => (
            format!(
                "SELECT {} FROM project_board_columns WHERE project_id = ?1 ORDER BY position ASC, created_at ASC",
                COLUMN_SELECT
            ),
            vec![&project_id as &dyn rusqlite::ToSql],
        ),
    };
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let items = stmt
        .query_map(params_vec.as_slice(), map_project_board_column)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(items)
}

pub fn get_default_column_sync(
    conn: &rusqlite::Connection,
    project_id: &str,
    board_id: Option<&str>,
) -> Result<Option<ProjectBoardColumn>, String> {
    let effective_board_id = match board_id {
        Some(id) => Some(id.to_string()),
        None => get_default_board_sync(conn, project_id)?.map(|b| b.id),
    };
    match effective_board_id {
        Some(board) => conn
            .query_row(
                &format!(
                    "SELECT {} FROM project_board_columns WHERE project_id = ?1 AND board_id = ?2 AND is_default = 1 LIMIT 1",
                    COLUMN_SELECT
                ),
                params![project_id, board],
                map_project_board_column,
            )
            .optional()
            .map_err(|e| e.to_string()),
        None => conn
            .query_row(
                &format!(
                    "SELECT {} FROM project_board_columns WHERE project_id = ?1 AND is_default = 1 LIMIT 1",
                    COLUMN_SELECT
                ),
                params![project_id],
                map_project_board_column,
            )
            .optional()
            .map_err(|e| e.to_string()),
    }
}

pub fn get_column_by_role_sync(
    conn: &rusqlite::Connection,
    project_id: &str,
    board_id: Option<&str>,
    role: &str,
) -> Result<Option<ProjectBoardColumn>, String> {
    let effective_board_id = match board_id {
        Some(id) => Some(id.to_string()),
        None => get_default_board_sync(conn, project_id)?.map(|b| b.id),
    };
    match effective_board_id {
        Some(board) => conn
            .query_row(
                &format!(
                    "SELECT {} FROM project_board_columns WHERE project_id = ?1 AND board_id = ?2 AND role = ?3 ORDER BY position ASC LIMIT 1",
                    COLUMN_SELECT
                ),
                params![project_id, board, role],
                map_project_board_column,
            )
            .optional()
            .map_err(|e| e.to_string()),
        None => conn
            .query_row(
                &format!(
                    "SELECT {} FROM project_board_columns WHERE project_id = ?1 AND role = ?2 ORDER BY position ASC LIMIT 1",
                    COLUMN_SELECT
                ),
                params![project_id, role],
                map_project_board_column,
            )
            .optional()
            .map_err(|e| e.to_string()),
    }
}

pub fn get_column_by_id_sync(
    conn: &rusqlite::Connection,
    id: &str,
) -> Result<Option<ProjectBoardColumn>, String> {
    conn.query_row(
        &format!(
            "SELECT {} FROM project_board_columns WHERE id = ?1",
            COLUMN_SELECT
        ),
        params![id],
        map_project_board_column,
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn current_board_revision_sync(
    conn: &rusqlite::Connection,
    project_id: &str,
) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT MAX(updated_at) FROM project_board_columns WHERE project_id = ?1",
        params![project_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
    .map(|value| value.flatten())
}

fn ensure_expected_revision(
    conn: &rusqlite::Connection,
    project_id: &str,
    expected_revision: Option<&str>,
) -> Result<(), String> {
    if let Some(expected_revision) = expected_revision {
        let current = current_board_revision_sync(conn, project_id)?;
        if current.as_deref() != Some(expected_revision) {
            return Err(
                "board columns changed since you loaded them; refresh and try again".into(),
            );
        }
    }
    Ok(())
}

pub fn resolve_board_column_sync(
    conn: &rusqlite::Connection,
    project_id: &str,
    board_id: Option<&str>,
    column_id: Option<&str>,
    status: Option<&str>,
) -> Result<ProjectBoardColumn, String> {
    if let Some(column_id) = column_id {
        let column = get_column_by_id_sync(conn, column_id)?
            .ok_or_else(|| format!("board column '{}' not found", column_id))?;
        if column.project_id != project_id {
            return Err(format!(
                "board column '{}' does not belong to project '{}'",
                column_id, project_id
            ));
        }
        if let Some(expected_board) = board_id {
            if column.board_id != expected_board {
                return Err(format!(
                    "board column '{}' does not belong to board '{}'",
                    column_id, expected_board
                ));
            }
        }
        if let Some(status) = status {
            validate_board_role(Some(status))?;
            if let Some(role) = column.role.as_deref() {
                if role != status {
                    return Err(format!(
                        "board column '{}' has role '{}' which does not match status '{}'",
                        column_id, role, status
                    ));
                }
            }
        }
        return Ok(column);
    }

    if let Some(status) = status {
        validate_board_role(Some(status))?;
        return get_column_by_role_sync(conn, project_id, board_id, status)?.ok_or_else(|| {
            format!(
                "project '{}' has no board column for role '{}'",
                project_id, status
            )
        });
    }

    if let Some(default_column) = get_default_column_sync(conn, project_id, board_id)? {
        return Ok(default_column);
    }

    list_project_board_columns_sync(conn, project_id, board_id)?
        .into_iter()
        .next()
        .ok_or_else(|| format!("project '{}' has no board columns", project_id))
}

fn normalize_default_candidate(
    conn: &rusqlite::Connection,
    project_id: &str,
    candidate_id: &str,
) -> Result<ProjectBoardColumn, String> {
    let candidate = require_project_column_sync(conn, project_id, candidate_id)?;
    if is_terminal_role(candidate.role.as_deref()) {
        return Err("default column cannot use a terminal role".into());
    }
    Ok(candidate)
}

fn set_default_column_sync(
    conn: &rusqlite::Connection,
    project_id: &str,
    board_id: &str,
    column_id: &str,
    now: &str,
) -> Result<(), String> {
    normalize_default_candidate(conn, project_id, column_id)?;
    conn.execute(
        "UPDATE project_board_columns
            SET is_default = CASE WHEN id = ?1 THEN 1 ELSE 0 END,
                updated_at = ?2
          WHERE project_id = ?3 AND board_id = ?4",
        params![column_id, now, project_id, board_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn list_referencing_workflows_sync(
    conn: &rusqlite::Connection,
    project_id: &str,
    column_id: &str,
) -> Result<Vec<(String, String)>, String> {
    let mut stmt = conn
        .prepare("SELECT id, name, graph FROM project_workflows WHERE project_id = ?1")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut refs = Vec::new();
    for row in rows {
        let (id, name, graph_str) = row.map_err(|e| e.to_string())?;
        let graph: Value = serde_json::from_str(&graph_str).unwrap_or(Value::Null);
        let matches = graph
            .get("nodes")
            .and_then(Value::as_array)
            .map(|nodes| {
                nodes.iter().any(|node| {
                    let data = node.get("data").and_then(Value::as_object);
                    match data {
                        Some(data) => {
                            data.get("columnId").and_then(Value::as_str) == Some(column_id)
                                || data.get("reviewColumnId").and_then(Value::as_str)
                                    == Some(column_id)
                                || data.get("listColumnId").and_then(Value::as_str)
                                    == Some(column_id)
                        }
                        None => false,
                    }
                })
            })
            .unwrap_or(false);
        if matches {
            refs.push((id, name));
        }
    }
    Ok(refs)
}

fn append_reassigned_items_sync(
    tx: &rusqlite::Transaction<'_>,
    source_column_id: &str,
    destination_column_id: &str,
    now: &str,
) -> Result<(), String> {
    let mut stmt = tx
        .prepare(
            "SELECT id FROM work_items WHERE column_id = ?1 ORDER BY position ASC, created_at ASC",
        )
        .map_err(|e| e.to_string())?;
    let item_ids = stmt
        .query_map(params![source_column_id], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>();
    let mut next_position: f64 = tx
        .query_row(
            "SELECT COALESCE(MAX(position), 0) FROM work_items WHERE column_id = ?1",
            params![destination_column_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    for item_id in item_ids {
        next_position += 1024.0;
        tx.execute(
            "UPDATE work_items SET column_id = ?1, position = ?2, updated_at = ?3 WHERE id = ?4",
            params![destination_column_id, next_position, now, item_id],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn list_project_board_columns(
    project_id: String,
    board_id: Option<String>,
    app: tauri::State<'_, AppContext>,
) -> Result<Vec<ProjectBoardColumn>, String> {
    app.repos
        .project_board_columns()
        .list(&project_id, board_id)
        .await
}

#[tauri::command]
pub async fn create_project_board_column(
    payload: CreateProjectBoardColumn,
    app: tauri::State<'_, AppContext>,
) -> Result<ProjectBoardColumn, String> {
    create_project_board_column_inner(payload, &app).await
}

async fn create_project_board_column_inner(
    payload: CreateProjectBoardColumn,
    app: &AppContext,
) -> Result<ProjectBoardColumn, String> {
    validate_board_role(payload.role.as_deref())?;
    let cloud = app.cloud.clone();
    let pool = app.db.0.clone();
    let column = tokio::task::spawn_blocking(move || -> Result<ProjectBoardColumn, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        if payload.name.trim().is_empty() {
            return Err("board column name must be non-empty".into());
        }
        if payload.is_default.unwrap_or(false) && is_terminal_role(payload.role.as_deref()) {
            return Err("default column cannot use a terminal role".into());
        }
        let board =
            resolve_board_sync(&conn, &payload.project_id, payload.board_id.as_deref())?;
        let board_id = board.id.clone();
        let id = Ulid::new().to_string();
        let position = match payload.position {
            Some(value) => value,
            None => {
                let max: Option<f64> = conn
                    .query_row(
                        "SELECT MAX(position) FROM project_board_columns WHERE project_id = ?1 AND board_id = ?2",
                        params![payload.project_id, board_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?
                    .flatten();
                max.unwrap_or(0.0) + 1024.0
            }
        };
        let has_default: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM project_board_columns WHERE project_id = ?1 AND board_id = ?2 AND is_default = 1)",
                params![payload.project_id, board_id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        let is_default = payload.is_default.unwrap_or(!has_default);
        if is_default && is_terminal_role(payload.role.as_deref()) {
            return Err("default column cannot use a terminal role".into());
        }
        if is_default {
            conn.execute(
                "UPDATE project_board_columns SET is_default = 0, updated_at = ?1 WHERE project_id = ?2 AND board_id = ?3",
                params![now, payload.project_id, board_id],
            )
            .map_err(|e| e.to_string())?;
        }
        conn.execute(
            "INSERT INTO project_board_columns (
                id, project_id, board_id, name, status, role, is_default, position, created_at, updated_at, tenant_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, COALESCE((SELECT tenant_id FROM projects WHERE id = ?2), 'local'))",
            params![
                id,
                payload.project_id,
                board_id,
                payload.name.trim(),
                payload.role.as_deref().unwrap_or("backlog"),
                payload.role,
                is_default,
                position,
                now,
            ],
        )
        .map_err(|e| e.to_string())?;
        conn.query_row(
            &format!("SELECT {} FROM project_board_columns WHERE id = ?1", COLUMN_SELECT),
            params![id],
            map_project_board_column,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_upsert_board_column!(cloud, column);
    Ok(column)
}

#[tauri::command]
pub async fn update_project_board_column(
    id: String,
    payload: UpdateProjectBoardColumn,
    app: tauri::State<'_, AppContext>,
) -> Result<ProjectBoardColumn, String> {
    update_project_board_column_inner(id, payload, &app).await
}

async fn update_project_board_column_inner(
    id: String,
    payload: UpdateProjectBoardColumn,
    app: &AppContext,
) -> Result<ProjectBoardColumn, String> {
    validate_board_role(payload.role.as_ref().and_then(|role| role.as_deref()))?;
    let cloud = app.cloud.clone();
    let pool = app.db.0.clone();
    let column = tokio::task::spawn_blocking(move || -> Result<ProjectBoardColumn, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let existing = get_column_by_id_sync(&conn, &id)?
            .ok_or_else(|| format!("board column '{}' not found", id))?;
        ensure_expected_revision(
            &conn,
            &existing.project_id,
            payload.expected_revision.as_deref(),
        )?;
        let now = chrono::Utc::now().to_rfc3339();
        if let Some(name) = payload.name.as_ref() {
            if name.trim().is_empty() {
                return Err("board column name must be non-empty".into());
            }
            conn.execute(
                "UPDATE project_board_columns SET name = ?1, updated_at = ?2 WHERE id = ?3",
                params![name.trim(), now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(position) = payload.position {
            conn.execute(
                "UPDATE project_board_columns SET position = ?1, updated_at = ?2 WHERE id = ?3",
                params![position, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(role) = payload.role.as_ref() {
            if existing.is_default && is_terminal_role(role.as_deref()) {
                return Err("default column cannot use a terminal role".into());
            }
            conn.execute(
                "UPDATE project_board_columns SET role = ?1, status = COALESCE(?1, status), updated_at = ?2 WHERE id = ?3",
                params![role, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if payload.is_default == Some(true) {
            set_default_column_sync(&conn, &existing.project_id, &existing.board_id, &id, &now)?;
        } else if payload.is_default == Some(false) && existing.is_default {
            return Err("choose another default column before unsetting the current default".into());
        }
        conn.query_row(
            &format!("SELECT {} FROM project_board_columns WHERE id = ?1", COLUMN_SELECT),
            params![id],
            map_project_board_column,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_upsert_board_column!(cloud, column);
    Ok(column)
}

#[tauri::command]
pub async fn delete_project_board_column(
    id: String,
    payload: DeleteProjectBoardColumn,
    app: tauri::State<'_, AppContext>,
) -> Result<(), String> {
    delete_project_board_column_inner(id, payload, &app).await
}

async fn delete_project_board_column_inner(
    id: String,
    payload: DeleteProjectBoardColumn,
    app: &AppContext,
) -> Result<(), String> {
    let cloud = app.cloud.clone();
    let pool = app.db.0.clone();
    let deleted_id = id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut conn = pool.get().map_err(|e| e.to_string())?;
        let existing = get_column_by_id_sync(&conn, &id)?
            .ok_or_else(|| format!("board column '{}' not found", id))?;
        ensure_expected_revision(
            &conn,
            &existing.project_id,
            payload.expected_revision.as_deref(),
        )?;
        let columns =
            list_project_board_columns_sync(&conn, &existing.project_id, Some(&existing.board_id))?;
        if columns.len() <= 1 {
            return Err("cannot delete the last remaining board column".into());
        }
        let refs = list_referencing_workflows_sync(&conn, &existing.project_id, &id)?;
        if !refs.is_empty() && !payload.force.unwrap_or(false) {
            let names = refs.into_iter().map(|(_, name)| name).collect::<Vec<_>>().join(", ");
            return Err(format!(
                "board column is referenced by workflows: {}. Retry with force to delete anyway.",
                names
            ));
        }

        let source_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM work_items WHERE column_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;

        let destination = match payload.destination_column_id.as_deref() {
            Some(destination_id) => Some(require_project_column_sync(
                &conn,
                &existing.project_id,
                destination_id,
            )?),
            None => None,
        };

        if source_count > 0 && destination.is_none() {
            return Err("choose a destination column before deleting a populated column".into());
        }

        if existing.is_default {
            if let Some(destination) = destination.as_ref() {
                if is_terminal_role(destination.role.as_deref()) {
                    return Err("default column cannot use a terminal role".into());
                }
            }
        }

        let default_destination = if existing.is_default {
            Some(destination.unwrap_or_else(|| {
                columns
                    .iter()
                    .find(|column| column.id != id && !is_terminal_role(column.role.as_deref()))
                    .cloned()
                    .unwrap_or_else(|| {
                        columns
                            .iter()
                            .find(|column| column.id != id)
                            .cloned()
                            .expect("at least one remaining board column")
                    })
            }))
        } else {
            destination
        };

        let now = chrono::Utc::now().to_rfc3339();
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        if source_count > 0 {
            let destination_id = default_destination
                .as_ref()
                .map(|column| column.id.as_str())
                .ok_or_else(|| "choose a destination column before deleting a populated column".to_string())?;
            append_reassigned_items_sync(&tx, &id, destination_id, &now)?;
        }
        if let Some(default_destination) = default_destination.as_ref() {
            tx.execute(
                "UPDATE project_board_columns SET is_default = CASE WHEN id = ?1 THEN 1 ELSE 0 END, updated_at = ?2 WHERE project_id = ?3 AND board_id = ?4",
                params![default_destination.id, now, existing.project_id, existing.board_id],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.execute(
            "DELETE FROM project_board_columns WHERE id = ?1",
            params![id],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_delete!(cloud, "project_board_columns", deleted_id);
    Ok(())
}

#[tauri::command]
pub async fn reorder_project_board_columns(
    project_id: String,
    payload: ReorderProjectBoardColumns,
    app: tauri::State<'_, AppContext>,
) -> Result<Vec<ProjectBoardColumn>, String> {
    reorder_project_board_columns_inner(project_id, payload, &app).await
}

async fn reorder_project_board_columns_inner(
    project_id: String,
    payload: ReorderProjectBoardColumns,
    app: &AppContext,
) -> Result<Vec<ProjectBoardColumn>, String> {
    let cloud = app.cloud.clone();
    let pool = app.db.0.clone();
    let columns =
        tokio::task::spawn_blocking(move || -> Result<Vec<ProjectBoardColumn>, String> {
            let mut conn = pool.get().map_err(|e| e.to_string())?;
            ensure_expected_revision(&conn, &project_id, payload.expected_revision.as_deref())?;
            let board = resolve_board_sync(&conn, &project_id, payload.board_id.as_deref())?;
            let existing = list_project_board_columns_sync(&conn, &project_id, Some(&board.id))?;
            if existing.len() != payload.ordered_ids.len() {
                return Err("reorder must include every board column exactly once".into());
            }
            let mut existing_ids = existing
                .iter()
                .map(|column| column.id.clone())
                .collect::<Vec<_>>();
            let mut ordered_ids = payload.ordered_ids.clone();
            existing_ids.sort();
            ordered_ids.sort();
            if existing_ids != ordered_ids {
                return Err("reorder payload does not match the board's columns".into());
            }
            let now = chrono::Utc::now().to_rfc3339();
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            for (idx, column_id) in payload.ordered_ids.iter().enumerate() {
                tx.execute(
                    "UPDATE project_board_columns SET position = ?1, updated_at = ?2 WHERE id = ?3",
                    params![((idx + 1) as f64) * 1024.0, now, column_id],
                )
                .map_err(|e| e.to_string())?;
            }
            tx.commit().map_err(|e| e.to_string())?;
            list_project_board_columns_sync(&conn, &project_id, Some(&board.id))
        })
        .await
        .map_err(|e| e.to_string())??;

    for column in &columns {
        cloud_upsert_board_column!(cloud, column);
    }
    Ok(columns)
}

mod http {
    use super::*;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ListArgs {
        project_id: String,
        #[serde(default)]
        board_id: Option<String>,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CreateArgs {
        payload: CreateProjectBoardColumn,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UpdateArgs {
        id: String,
        payload: UpdateProjectBoardColumn,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct DeleteArgs {
        id: String,
        payload: DeleteProjectBoardColumn,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ReorderArgs {
        project_id: String,
        payload: ReorderProjectBoardColumns,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("list_project_board_columns", |ctx, args| async move {
            let a: ListArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx
                .repos
                .project_board_columns()
                .list(&a.project_id, a.board_id)
                .await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("create_project_board_column", |ctx, args| async move {
            let a: CreateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = create_project_board_column_inner(a.payload, &ctx).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("update_project_board_column", |ctx, args| async move {
            let a: UpdateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = update_project_board_column_inner(a.id, a.payload, &ctx).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("delete_project_board_column", |ctx, args| async move {
            let a: DeleteArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            delete_project_board_column_inner(a.id, a.payload, &ctx).await?;
            Ok(serde_json::Value::Null)
        });
        reg.register("reorder_project_board_columns", |ctx, args| async move {
            let a: ReorderArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = reorder_project_board_columns_inner(a.project_id, a.payload, &ctx).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
    }
}

pub use http::register as register_http;
