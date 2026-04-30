use std::path::PathBuf;

use crate::db::DbPool;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionWorktreeState {
    pub name: String,
    pub branch: String,
    pub path: PathBuf,
}

pub async fn load_session_worktree_state(
    db: &DbPool,
    session_id: &str,
) -> Result<Option<SessionWorktreeState>, String> {
    let pool = db.0.clone();
    let session_id = session_id.to_string();

    tokio::task::spawn_blocking(move || -> Result<Option<SessionWorktreeState>, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let row: (Option<String>, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT worktree_name, worktree_branch, worktree_path
                 FROM chat_sessions
                 WHERE id = ?1
                   AND tenant_id = COALESCE((SELECT tenant_id FROM chat_sessions WHERE id = ?1), 'local')",
                rusqlite::params![session_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|e| e.to_string())?;

        match row {
            (Some(name), Some(branch), Some(path)) => Ok(Some(SessionWorktreeState {
                name,
                branch,
                path: PathBuf::from(path),
            })),
            _ => Ok(None),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn load_session_project_id(
    db: &DbPool,
    session_id: &str,
) -> Result<Option<String>, String> {
    let pool = db.0.clone();
    let session_id = session_id.to_string();

    tokio::task::spawn_blocking(move || -> Result<Option<String>, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let project_id: Option<String> = conn
            .query_row(
                "SELECT project_id
                   FROM chat_sessions
                  WHERE id = ?1
                    AND tenant_id = COALESCE((SELECT tenant_id FROM chat_sessions WHERE id = ?1), 'local')",
                rusqlite::params![session_id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        Ok(project_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn set_session_worktree_state(
    db: &DbPool,
    session_id: &str,
    state: Option<&SessionWorktreeState>,
) -> Result<(), String> {
    let pool = db.0.clone();
    let session_id = session_id.to_string();
    let state = state.cloned();

    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        let (name, branch, path) = match state {
            Some(state) => (
                Some(state.name),
                Some(state.branch),
                Some(state.path.to_string_lossy().to_string()),
            ),
            None => (None, None, None),
        };

        conn.execute(
            "UPDATE chat_sessions
             SET worktree_name = ?1,
                 worktree_branch = ?2,
                 worktree_path = ?3,
                 updated_at = ?4
             WHERE id = ?5
               AND tenant_id = COALESCE((SELECT tenant_id FROM chat_sessions WHERE id = ?5), 'local')",
            rusqlite::params![name, branch, path, now, session_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
