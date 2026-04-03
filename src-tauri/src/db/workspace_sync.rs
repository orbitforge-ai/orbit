//! Workspace file sync via Supabase Storage + workspace_objects manifest.
//!
//! Architecture:
//! - Local write → hash → Storage upload → Postgres manifest upsert → local SQLite manifest
//! - Local delete → Storage delete → Postgres tombstone → local SQLite tombstone
//! - Realtime workspace_objects INSERT/UPDATE → download from Storage → write to disk
//! - Conflict resolution: last-write-wins by `version` (Unix ms timestamp).

use std::sync::Arc;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tracing::warn;

use crate::db::cloud::SupabaseClient;

const BUCKET: &str = "orbit-workspaces";

// ---------------------------------------------------------------------------
// Public entry points — called fire-and-forget from commands
// ---------------------------------------------------------------------------

/// Push an agent workspace file to Storage + manifest.
/// `path_from_agent_root` is relative to `~/.orbit/agents/{agent_id}/`
/// (e.g. `"workspace/notes.md"`). Files not under `workspace/` are skipped.
pub async fn push_agent_file(
    client: &Arc<SupabaseClient>,
    pool: &Pool<SqliteConnectionManager>,
    agent_id: &str,
    path_from_agent_root: &str,
) {
    let ws_path = match strip_workspace_prefix(path_from_agent_root) {
        Some(p) => p,
        None => return, // not under workspace/ — skip silently
    };
    let local = crate::executor::workspace::agent_dir(agent_id)
        .join("workspace")
        .join(&ws_path);
    if let Err(e) = push_file(client, pool, "agent", agent_id, &ws_path, &local).await {
        warn!(
            "workspace_sync push_agent_file {}/{}: {}",
            agent_id, ws_path, e
        );
    }
}

/// Tombstone an agent workspace file/directory in Storage + manifest.
pub async fn delete_agent_file(
    client: &Arc<SupabaseClient>,
    pool: &Pool<SqliteConnectionManager>,
    agent_id: &str,
    path_from_agent_root: &str,
) {
    let ws_path = match strip_workspace_prefix(path_from_agent_root) {
        Some(p) => p,
        None => return,
    };
    if let Err(e) = delete_file(client, pool, "agent", agent_id, &ws_path).await {
        warn!(
            "workspace_sync delete_agent_file {}/{}: {}",
            agent_id, ws_path, e
        );
    }
}

/// Push a project workspace file to Storage + manifest.
/// `path` is relative to `~/.orbit/projects/{project_id}/workspace/`.
pub async fn push_project_file(
    client: &Arc<SupabaseClient>,
    pool: &Pool<SqliteConnectionManager>,
    project_id: &str,
    path: &str,
) {
    let local = crate::executor::workspace::project_workspace_dir(project_id).join(path);
    if let Err(e) = push_file(client, pool, "project", project_id, path, &local).await {
        warn!(
            "workspace_sync push_project_file {}/{}: {}",
            project_id, path, e
        );
    }
}

/// Tombstone a project workspace file/directory in Storage + manifest.
pub async fn delete_project_file(
    client: &Arc<SupabaseClient>,
    pool: &Pool<SqliteConnectionManager>,
    project_id: &str,
    path: &str,
) {
    if let Err(e) = delete_file(client, pool, "project", project_id, path).await {
        warn!(
            "workspace_sync delete_project_file {}/{}: {}",
            project_id, path, e
        );
    }
}

/// Apply a remote workspace_objects change received via Realtime.
/// Downloads the file from Storage if it is newer than what is on disk.
pub async fn apply_remote_workspace_change(
    client: &Arc<SupabaseClient>,
    pool: &Pool<SqliteConnectionManager>,
    record: &Value,
) {
    if let Err(e) = do_apply_remote(client, pool, record).await {
        warn!("workspace_sync apply_remote: {}", e);
    }
}

// ---------------------------------------------------------------------------
// Core implementation
// ---------------------------------------------------------------------------

async fn push_file(
    client: &Arc<SupabaseClient>,
    pool: &Pool<SqliteConnectionManager>,
    scope_type: &str,
    scope_id: &str,
    ws_path: &str,
    local: &std::path::Path,
) -> Result<(), String> {
    // Skip if it's a directory (only sync files)
    if local.is_dir() {
        return Ok(());
    }

    let bytes = std::fs::read(local).map_err(|e| format!("read {}: {e}", local.display()))?;
    let sha256 = hex_sha256(&bytes);
    let size_bytes = bytes.len() as i64;
    let version = chrono::Utc::now().timestamp_millis();
    let storage_path = storage_key(&client.user_id, scope_type, scope_id, ws_path);
    let updated_at = chrono::Utc::now().to_rfc3339();
    let mime = detect_mime(ws_path);

    // Upload bytes to Storage
    client.storage_upload(BUCKET, &storage_path, &bytes).await?;

    // Upsert manifest in Postgres
    let manifest_row = serde_json::json!({
        "user_id": client.user_id,
        "scope_type": scope_type,
        "scope_id": scope_id,
        "path": ws_path,
        "storage_path": storage_path,
        "sha256": sha256,
        "size_bytes": size_bytes,
        "mime_type": mime,
        "version": version,
        "deleted_at": null,
        "updated_at": updated_at,
    });
    client.upsert_single("workspace_objects", manifest_row).await?;

    // Update local SQLite manifest
    let pool = pool.clone();
    let user_id = client.user_id.clone();
    let scope_type = scope_type.to_string();
    let scope_id = scope_id.to_string();
    let ws_path = ws_path.to_string();
    let storage_path = storage_path.to_string();
    let sha256 = sha256.to_string();
    let mime = mime.to_string();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        upsert_local_manifest(
            &conn,
            &user_id,
            &scope_type,
            &scope_id,
            &ws_path,
            &storage_path,
            &sha256,
            size_bytes,
            &mime,
            version,
            None,
            &updated_at,
        )
    })
    .await
    .map_err(|e| e.to_string())??;

    Ok(())
}

async fn delete_file(
    client: &Arc<SupabaseClient>,
    pool: &Pool<SqliteConnectionManager>,
    scope_type: &str,
    scope_id: &str,
    ws_path: &str,
) -> Result<(), String> {
    let deleted_at = chrono::Utc::now().to_rfc3339();
    let version = chrono::Utc::now().timestamp_millis();
    let user_id = client.user_id.clone();

    // Collect all affected paths from local manifest (handles directories too)
    let pool_clone = pool.clone();
    let scope_type_s = scope_type.to_string();
    let scope_id_s = scope_id.to_string();
    let ws_path_s = ws_path.to_string();
    let user_id_s = user_id.clone();

    let paths: Vec<(String, String)> = tokio::task::spawn_blocking(move || {
        let conn = pool_clone.get().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT path, storage_path FROM workspace_objects
                 WHERE user_id=?1 AND scope_type=?2 AND scope_id=?3
                   AND path=?4 OR path LIKE ?5
                   AND deleted_at IS NULL",
            )
            .map_err(|e| e.to_string())?;
        let prefix_pattern = format!("{}/", ws_path_s);
        let rows: Vec<(String, String)> = stmt
            .query_map(
                rusqlite::params![
                    user_id_s,
                    scope_type_s,
                    scope_id_s,
                    ws_path_s,
                    prefix_pattern
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok::<_, String>(rows)
    })
    .await
    .map_err(|e| e.to_string())??;

    if paths.is_empty() {
        // No manifest entry — nothing to tombstone
        return Ok(());
    }

    // Delete from Storage
    let storage_paths: Vec<String> = paths.iter().map(|(_, sp)| sp.clone()).collect();
    if let Err(e) = client.storage_delete(BUCKET, storage_paths).await {
        warn!("Storage delete failed (continuing): {}", e);
    }

    // Tombstone in Postgres
    for (path, storage_path) in &paths {
        let tombstone = serde_json::json!({
            "user_id": user_id,
            "scope_type": scope_type,
            "scope_id": scope_id,
            "path": path,
            "storage_path": storage_path,
            "sha256": "",
            "size_bytes": 0,
            "version": version,
            "deleted_at": deleted_at,
            "updated_at": deleted_at,
        });
        if let Err(e) = client.upsert_single("workspace_objects", tombstone).await {
            warn!("Tombstone Postgres {}/{}: {}", scope_id, path, e);
        }
    }

    // Tombstone in local SQLite
    let pool_clone = pool.clone();
    let user_id_s = user_id.clone();
    let scope_type_s = scope_type.to_string();
    let scope_id_s = scope_id.to_string();
    let ws_path_s = ws_path.to_string();
    let deleted_at_s = deleted_at.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool_clone.get().map_err(|e| e.to_string())?;
        let prefix_pattern = format!("{}/", ws_path_s);
        conn.execute(
            "UPDATE workspace_objects SET deleted_at=?1, version=?2, updated_at=?1
             WHERE user_id=?3 AND scope_type=?4 AND scope_id=?5
               AND (path=?6 OR path LIKE ?7)",
            rusqlite::params![
                deleted_at_s,
                version,
                user_id_s,
                scope_type_s,
                scope_id_s,
                ws_path_s,
                prefix_pattern,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())??;

    Ok(())
}

async fn do_apply_remote(
    client: &Arc<SupabaseClient>,
    pool: &Pool<SqliteConnectionManager>,
    record: &Value,
) -> Result<(), String> {
    let scope_type = record
        .get("scope_type")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let scope_id = record
        .get("scope_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let path = record
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let storage_path = record
        .get("storage_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let remote_version = record
        .get("version")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let deleted_at = record.get("deleted_at").and_then(|v| v.as_str());

    if scope_type.is_empty() || scope_id.is_empty() || path.is_empty() {
        return Ok(());
    }

    // Determine local file path
    let local = match scope_type {
        "agent" => crate::executor::workspace::agent_dir(scope_id)
            .join("workspace")
            .join(path),
        "project" => {
            crate::executor::workspace::project_workspace_dir(scope_id).join(path)
        }
        _ => return Err(format!("unknown scope_type: {scope_type}")),
    };

    // Check current local version from manifest
    let pool_clone = pool.clone();
    let scope_type_s = scope_type.to_string();
    let scope_id_s = scope_id.to_string();
    let path_s = path.to_string();
    let user_id_s = client.user_id.clone();

    let local_version: i64 = tokio::task::spawn_blocking(move || {
        let conn = pool_clone.get().map_err(|e| e.to_string())?;
        let v: i64 = conn
            .query_row(
                "SELECT version FROM workspace_objects
                 WHERE user_id=?1 AND scope_type=?2 AND scope_id=?3 AND path=?4",
                rusqlite::params![user_id_s, scope_type_s, scope_id_s, path_s],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok::<i64, String>(v)
    })
    .await
    .map_err(|e| e.to_string())??;

    // Skip if we already have this or a newer version
    if remote_version <= local_version {
        return Ok(());
    }

    if deleted_at.is_some() {
        // Tombstone: remove local file
        if local.exists() {
            if local.is_dir() {
                std::fs::remove_dir_all(&local)
                    .map_err(|e| format!("remove dir {}: {e}", local.display()))?;
            } else {
                std::fs::remove_file(&local)
                    .map_err(|e| format!("remove file {}: {e}", local.display()))?;
            }
        }
    } else {
        // Download and write
        if storage_path.is_empty() {
            return Err("empty storage_path in record".to_string());
        }
        let bytes = client.storage_download(BUCKET, storage_path).await?;
        if let Some(parent) = local.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create dirs: {e}"))?;
        }
        std::fs::write(&local, &bytes)
            .map_err(|e| format!("write {}: {e}", local.display()))?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// SQLite manifest helper
// ---------------------------------------------------------------------------

fn upsert_local_manifest(
    conn: &rusqlite::Connection,
    user_id: &str,
    scope_type: &str,
    scope_id: &str,
    path: &str,
    storage_path: &str,
    sha256: &str,
    size_bytes: i64,
    mime: &str,
    version: i64,
    deleted_at: Option<&str>,
    updated_at: &str,
) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO workspace_objects
         (user_id, scope_type, scope_id, path, storage_path, sha256,
          size_bytes, mime_type, version, deleted_at, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        rusqlite::params![
            user_id,
            scope_type,
            scope_id,
            path,
            storage_path,
            sha256,
            size_bytes,
            mime,
            version,
            deleted_at,
            updated_at,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn strip_workspace_prefix(path: &str) -> Option<String> {
    let p = path.trim_start_matches('/');
    if p == "workspace" {
        // Bare directory — nothing to sync
        return None;
    }
    p.strip_prefix("workspace/").map(|s| s.to_string())
}

fn storage_key(user_id: &str, scope_type: &str, scope_id: &str, ws_path: &str) -> String {
    format!(
        "users/{}/{}s/{}/workspace/{}",
        user_id, scope_type, scope_id, ws_path
    )
}

fn hex_sha256(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

fn detect_mime(path: &str) -> String {
    match path.rsplit('.').next().unwrap_or("") {
        "md" | "markdown" => "text/markdown",
        "txt" => "text/plain",
        "json" => "application/json",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        "ts" => "application/typescript",
        "py" => "text/x-python",
        "rs" => "text/x-rust",
        "toml" => "application/toml",
        "yaml" | "yml" => "application/yaml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
    .to_string()
}
