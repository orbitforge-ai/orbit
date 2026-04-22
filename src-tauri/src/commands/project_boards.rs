use crate::db::cloud::CloudClientState;
use crate::db::DbPool;
use crate::models::project_board::{
    CreateProjectBoard, DeleteProjectBoard, ProjectBoard, UpdateProjectBoard,
};
use rusqlite::{params, OptionalExtension};
use ulid::Ulid;

const BOARD_SELECT: &str =
    "id, project_id, name, prefix, position, is_default, created_at, updated_at";

pub(crate) fn map_project_board(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectBoard> {
    Ok(ProjectBoard {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        prefix: row.get(3)?,
        position: row.get(4)?,
        is_default: row.get::<_, bool>(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

pub fn validate_board_prefix(prefix: &str) -> Result<(), String> {
    let trimmed = prefix.trim();
    if trimmed.len() < 2 || trimmed.len() > 8 {
        return Err("board prefix must be 2 to 8 characters long".into());
    }
    if !trimmed.chars().all(|c| c.is_ascii_uppercase()) {
        return Err("board prefix must contain only uppercase letters A–Z".into());
    }
    Ok(())
}

pub fn list_project_boards_sync(
    conn: &rusqlite::Connection,
    project_id: &str,
) -> Result<Vec<ProjectBoard>, String> {
    let sql = format!(
        "SELECT {} FROM project_boards WHERE project_id = ?1 ORDER BY is_default DESC, position ASC, created_at ASC",
        BOARD_SELECT
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let boards = stmt
        .query_map(params![project_id], map_project_board)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(boards)
}

pub fn get_board_by_id_sync(
    conn: &rusqlite::Connection,
    id: &str,
) -> Result<Option<ProjectBoard>, String> {
    conn.query_row(
        &format!("SELECT {} FROM project_boards WHERE id = ?1", BOARD_SELECT),
        params![id],
        map_project_board,
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn get_default_board_sync(
    conn: &rusqlite::Connection,
    project_id: &str,
) -> Result<Option<ProjectBoard>, String> {
    conn.query_row(
        &format!(
            "SELECT {} FROM project_boards WHERE project_id = ?1 AND is_default = 1 LIMIT 1",
            BOARD_SELECT
        ),
        params![project_id],
        map_project_board,
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// Resolve the board to use for a given project + optional explicit board_id.
/// Falls back to the project's default board, then the first board it finds.
pub fn resolve_board_sync(
    conn: &rusqlite::Connection,
    project_id: &str,
    board_id: Option<&str>,
) -> Result<ProjectBoard, String> {
    if let Some(board_id) = board_id {
        let board = get_board_by_id_sync(conn, board_id)?
            .ok_or_else(|| format!("board '{}' not found", board_id))?;
        if board.project_id != project_id {
            return Err(format!(
                "board '{}' does not belong to project '{}'",
                board_id, project_id
            ));
        }
        return Ok(board);
    }

    if let Some(default) = get_default_board_sync(conn, project_id)? {
        return Ok(default);
    }

    list_project_boards_sync(conn, project_id)?
        .into_iter()
        .next()
        .ok_or_else(|| format!("project '{}' has no boards", project_id))
}

#[tauri::command]
pub async fn list_project_boards(
    project_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<ProjectBoard>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        list_project_boards_sync(&conn, &project_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn create_project_board(
    payload: CreateProjectBoard,
    db: tauri::State<'_, DbPool>,
    _cloud: tauri::State<'_, CloudClientState>,
) -> Result<ProjectBoard, String> {
    validate_board_prefix(&payload.prefix)?;
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || -> Result<ProjectBoard, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let name = payload.name.trim();
        if name.is_empty() {
            return Err("board name must be non-empty".into());
        }
        let prefix = payload.prefix.trim().to_string();

        let prefix_taken: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM project_boards WHERE project_id = ?1 AND prefix = ?2)",
                params![payload.project_id, prefix],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if prefix_taken {
            return Err(format!(
                "a board with prefix '{}' already exists in this project",
                prefix
            ));
        }

        let now = chrono::Utc::now().to_rfc3339();
        let id = Ulid::new().to_string();

        let next_position: f64 = conn
            .query_row(
                "SELECT COALESCE(MAX(position), 0) FROM project_boards WHERE project_id = ?1",
                params![payload.project_id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        let position = next_position + 1024.0;

        conn.execute(
            "INSERT INTO project_boards (id, project_id, name, prefix, position, is_default, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?6)",
            params![id, payload.project_id, name, prefix, position, now],
        )
        .map_err(|e| e.to_string())?;

        conn.query_row(
            &format!("SELECT {} FROM project_boards WHERE id = ?1", BOARD_SELECT),
            params![id],
            map_project_board,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn update_project_board(
    id: String,
    payload: UpdateProjectBoard,
    db: tauri::State<'_, DbPool>,
    _cloud: tauri::State<'_, CloudClientState>,
) -> Result<ProjectBoard, String> {
    if let Some(prefix) = payload.prefix.as_deref() {
        validate_board_prefix(prefix)?;
    }
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || -> Result<ProjectBoard, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let existing = get_board_by_id_sync(&conn, &id)?
            .ok_or_else(|| format!("board '{}' not found", id))?;
        let now = chrono::Utc::now().to_rfc3339();

        if let Some(name) = payload.name.as_deref() {
            let name = name.trim();
            if name.is_empty() {
                return Err("board name must be non-empty".into());
            }
            conn.execute(
                "UPDATE project_boards SET name = ?1, updated_at = ?2 WHERE id = ?3",
                params![name, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(prefix) = payload.prefix.as_deref() {
            let prefix = prefix.trim().to_string();
            if prefix != existing.prefix {
                let taken: bool = conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM project_boards WHERE project_id = ?1 AND prefix = ?2 AND id != ?3)",
                        params![existing.project_id, prefix, id],
                        |row| row.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                if taken {
                    return Err(format!(
                        "a board with prefix '{}' already exists in this project",
                        prefix
                    ));
                }
                conn.execute(
                    "UPDATE project_boards SET prefix = ?1, updated_at = ?2 WHERE id = ?3",
                    params![prefix, now, id],
                )
                .map_err(|e| e.to_string())?;
            }
        }

        conn.query_row(
            &format!("SELECT {} FROM project_boards WHERE id = ?1", BOARD_SELECT),
            params![id],
            map_project_board,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_project_board(
    id: String,
    payload: DeleteProjectBoard,
    db: tauri::State<'_, DbPool>,
    _cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut conn = pool.get().map_err(|e| e.to_string())?;
        let existing = get_board_by_id_sync(&conn, &id)?
            .ok_or_else(|| format!("board '{}' not found", id))?;

        let siblings = list_project_boards_sync(&conn, &existing.project_id)?;
        if siblings.len() <= 1 {
            return Err("cannot delete the last remaining board".into());
        }

        let item_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM work_items WHERE board_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;

        let destination = match payload.destination_board_id.as_deref() {
            Some(dest_id) => {
                let dest = get_board_by_id_sync(&conn, dest_id)?
                    .ok_or_else(|| format!("destination board '{}' not found", dest_id))?;
                if dest.project_id != existing.project_id {
                    return Err("destination board belongs to a different project".into());
                }
                if dest.id == existing.id {
                    return Err("destination board must be different from the board being deleted"
                        .into());
                }
                Some(dest)
            }
            None => None,
        };

        if item_count > 0 && destination.is_none() && !payload.force.unwrap_or(false) {
            return Err(
                "choose a destination board before deleting a board that has items".into(),
            );
        }

        let now = chrono::Utc::now().to_rfc3339();
        let tx = conn.transaction().map_err(|e| e.to_string())?;

        if let Some(destination) = destination.as_ref() {
            // Re-parent every column and work item from source board to destination.
            tx.execute(
                "UPDATE project_board_columns SET board_id = ?1, updated_at = ?2 WHERE board_id = ?3",
                params![destination.id, now, id],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "UPDATE work_items SET board_id = ?1, updated_at = ?2 WHERE board_id = ?3",
                params![destination.id, now, id],
            )
            .map_err(|e| e.to_string())?;
        }

        // If we're deleting the default board, promote another board first so
        // the partial unique index stays consistent.
        if existing.is_default {
            let next_default = siblings
                .iter()
                .find(|b| b.id != id)
                .ok_or_else(|| "expected at least one remaining board".to_string())?;
            tx.execute(
                "UPDATE project_boards SET is_default = 0, updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "UPDATE project_boards SET is_default = 1, updated_at = ?1 WHERE id = ?2",
                params![now, next_default.id],
            )
            .map_err(|e| e.to_string())?;
        }

        tx.execute("DELETE FROM project_boards WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
