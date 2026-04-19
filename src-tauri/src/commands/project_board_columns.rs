use crate::db::DbPool;
use crate::models::project_board_column::{
    CreateProjectBoardColumn, ProjectBoardColumn, UpdateProjectBoardColumn,
};
use rusqlite::{params, OptionalExtension};
use ulid::Ulid;

pub const LEGACY_BOARD_STATUSES: &[&str] = &[
    "backlog",
    "todo",
    "in_progress",
    "blocked",
    "review",
    "done",
    "cancelled",
];

const COLUMN_SELECT: &str = "id, project_id, name, status, position, created_at, updated_at";

pub fn default_board_columns() -> [(&'static str, &'static str); 7] {
    [
        ("Backlog", "backlog"),
        ("Todo", "todo"),
        ("In Progress", "in_progress"),
        ("Blocked", "blocked"),
        ("Review", "review"),
        ("Done", "done"),
        ("Cancelled", "cancelled"),
    ]
}

pub(crate) fn map_project_board_column(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ProjectBoardColumn> {
    Ok(ProjectBoardColumn {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        status: row.get(3)?,
        position: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

pub fn validate_board_status(status: &str) -> Result<(), String> {
    if LEGACY_BOARD_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(format!("invalid board status '{}'", status))
    }
}

pub fn ensure_project_board_columns(
    conn: &rusqlite::Connection,
    project_id: &str,
    created_at: &str,
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

    for (idx, (name, status)) in default_board_columns().into_iter().enumerate() {
        conn.execute(
            "INSERT INTO project_board_columns (
                id, project_id, name, status, position, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                format!("col_{}_{}", project_id, status),
                project_id,
                name,
                status,
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
) -> Result<Vec<ProjectBoardColumn>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {} FROM project_board_columns WHERE project_id = ?1 ORDER BY position ASC, created_at ASC",
            COLUMN_SELECT
        ))
        .map_err(|e| e.to_string())?;
    let items = stmt
        .query_map(params![project_id], map_project_board_column)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(items)
}

pub fn get_column_by_status_sync(
    conn: &rusqlite::Connection,
    project_id: &str,
    status: &str,
) -> Result<Option<ProjectBoardColumn>, String> {
    conn.query_row(
        &format!(
            "SELECT {} FROM project_board_columns WHERE project_id = ?1 AND status = ?2 ORDER BY position ASC LIMIT 1",
            COLUMN_SELECT
        ),
        params![project_id, status],
        map_project_board_column,
    )
    .optional()
    .map_err(|e| e.to_string())
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

pub fn resolve_board_column_sync(
    conn: &rusqlite::Connection,
    project_id: &str,
    column_id: Option<&str>,
    status: Option<&str>,
) -> Result<ProjectBoardColumn, String> {
    if let Some(column_id) = column_id {
        if let Some(column) = get_column_by_id_sync(conn, column_id)? {
            if column.project_id != project_id {
                return Err(format!(
                    "board column '{}' does not belong to project '{}'",
                    column_id, project_id
                ));
            }
            return Ok(column);
        }
        return Err(format!("board column '{}' not found", column_id));
    }

    let status = status.unwrap_or("backlog");
    validate_board_status(status)?;
    if let Some(column) = get_column_by_status_sync(conn, project_id, status)? {
        return Ok(column);
    }

    let columns = list_project_board_columns_sync(conn, project_id)?;
    columns
        .into_iter()
        .next()
        .ok_or_else(|| format!("project '{}' has no board columns", project_id))
}

#[tauri::command]
pub async fn list_project_board_columns(
    project_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<ProjectBoardColumn>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        list_project_board_columns_sync(&conn, &project_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn create_project_board_column(
    payload: CreateProjectBoardColumn,
    db: tauri::State<'_, DbPool>,
) -> Result<ProjectBoardColumn, String> {
    validate_board_status(&payload.status)?;
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || -> Result<ProjectBoardColumn, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let id = Ulid::new().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let position = match payload.position {
            Some(value) => value,
            None => {
                let max: Option<f64> = conn
                    .query_row(
                        "SELECT MAX(position) FROM project_board_columns WHERE project_id = ?1",
                        params![payload.project_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?
                    .flatten();
                max.unwrap_or(0.0) + 1024.0
            }
        };
        conn.execute(
            "INSERT INTO project_board_columns (
                id, project_id, name, status, position, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                id,
                payload.project_id,
                payload.name,
                payload.status,
                position,
                now
            ],
        )
        .map_err(|e| e.to_string())?;

        conn.query_row(
            &format!(
                "SELECT {} FROM project_board_columns WHERE id = ?1",
                COLUMN_SELECT
            ),
            params![id],
            map_project_board_column,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn update_project_board_column(
    id: String,
    payload: UpdateProjectBoardColumn,
    db: tauri::State<'_, DbPool>,
) -> Result<ProjectBoardColumn, String> {
    if let Some(status) = payload.status.as_deref() {
        validate_board_status(status)?;
    }
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || -> Result<ProjectBoardColumn, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        if let Some(name) = payload.name.as_ref() {
            conn.execute(
                "UPDATE project_board_columns SET name = ?1, updated_at = ?2 WHERE id = ?3",
                params![name, now, id],
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
        if let Some(status) = payload.status.as_ref() {
            conn.execute(
                "UPDATE project_board_columns SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![status, now, id],
            )
            .map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE work_items
                    SET status = ?1, updated_at = ?2
                  WHERE column_id = ?3",
                params![status, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        conn.query_row(
            &format!(
                "SELECT {} FROM project_board_columns WHERE id = ?1",
                COLUMN_SELECT
            ),
            params![id],
            map_project_board_column,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
