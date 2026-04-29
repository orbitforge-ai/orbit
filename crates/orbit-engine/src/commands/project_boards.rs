//! Project-board commands.
//!
//! Read/write goes through `ProjectBoardRepo`. We keep a few `*_sync` helpers
//! exported because other modules call them while already holding a
//! `&Connection` open inside a wider transaction (e.g. `commands/work_items`,
//! `commands/projects::create_project`). Those helpers stay rusqlite-shaped
//! until the rest of those modules also move onto the trait surface.

use crate::app_context::AppContext;
use crate::db::cloud::CloudClientState;
use crate::models::project_board::{
    CreateProjectBoard, DeleteProjectBoard, ProjectBoard, UpdateProjectBoard,
};
use rusqlite::{params, OptionalExtension};

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

// ── In-transaction helpers used by other command modules ──────────────────

pub fn list_project_boards_sync(
    conn: &rusqlite::Connection,
    project_id: &str,
) -> Result<Vec<ProjectBoard>, String> {
    let sql = format!(
        "SELECT {} FROM project_boards
         WHERE project_id = ?1
           AND tenant_id = COALESCE((SELECT tenant_id FROM projects WHERE id = ?1), 'local')
         ORDER BY is_default DESC, position ASC, created_at ASC",
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
        &format!(
            "SELECT {} FROM project_boards
             WHERE id = ?1
               AND tenant_id = COALESCE((SELECT tenant_id FROM project_boards WHERE id = ?1), 'local')",
            BOARD_SELECT
        ),
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
            "SELECT {} FROM project_boards
             WHERE project_id = ?1
               AND tenant_id = COALESCE((SELECT tenant_id FROM projects WHERE id = ?1), 'local')
               AND is_default = 1
             LIMIT 1",
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

// ── Tauri commands ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_project_boards(
    project_id: String,
    app: tauri::State<'_, AppContext>,
) -> Result<Vec<ProjectBoard>, String> {
    app.repos.project_boards().list(&project_id).await
}

#[tauri::command]
pub async fn create_project_board(
    payload: CreateProjectBoard,
    app: tauri::State<'_, AppContext>,
    _cloud: tauri::State<'_, CloudClientState>,
) -> Result<ProjectBoard, String> {
    app.repos.project_boards().create(payload).await
}

#[tauri::command]
pub async fn update_project_board(
    id: String,
    payload: UpdateProjectBoard,
    app: tauri::State<'_, AppContext>,
    _cloud: tauri::State<'_, CloudClientState>,
) -> Result<ProjectBoard, String> {
    app.repos.project_boards().update(&id, payload).await
}

#[tauri::command]
pub async fn delete_project_board(
    id: String,
    payload: DeleteProjectBoard,
    app: tauri::State<'_, AppContext>,
    _cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
    app.repos.project_boards().delete(&id, payload).await
}

mod http {
    use super::*;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ProjectIdArgs {
        project_id: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CreateArgs {
        payload: CreateProjectBoard,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UpdateArgs {
        id: String,
        payload: UpdateProjectBoard,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct DeleteArgs {
        id: String,
        payload: DeleteProjectBoard,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("list_project_boards", |ctx, args| async move {
            let a: ProjectIdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx.repos.project_boards().list(&a.project_id).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("create_project_board", |ctx, args| async move {
            let a: CreateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx.repos.project_boards().create(a.payload).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("update_project_board", |ctx, args| async move {
            let a: UpdateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx.repos.project_boards().update(&a.id, a.payload).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("delete_project_board", |ctx, args| async move {
            let a: DeleteArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            ctx.repos.project_boards().delete(&a.id, a.payload).await?;
            Ok(serde_json::Value::Null)
        });
    }
}

pub use http::register as register_http;
