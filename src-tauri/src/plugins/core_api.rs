//! Unix-domain socket server per plugin. A plugin subprocess dials
//! `ORBIT_CORE_API_SOCKET` and exchanges newline-delimited JSON-RPC 2.0
//! requests with the core.
//!
//! Methods exposed in V1:
//!   - `entity.list`, `entity.get`, `entity.create`, `entity.update`,
//!     `entity.delete`, `entity.link`, `entity.unlink`, `entity.list_relations`
//!   - `work_item.get`, `work_item.list` (read-only; gated by
//!     `permissions.coreEntities` whitelist)
//!   - `workflow.fire_trigger` (for plugin-defined workflow triggers)
//!
//! Each plugin has its own socket at
//! `~/.orbit/plugins/<id>/.orbit/core.sock`. Permission 0600, bound only to
//! the path the plugin's subprocess knows.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::db::DbPool;

use super::entities;
use super::manifest::PluginManifest;

pub struct CoreApiServer {
    /// Tracks spawned sockets so shutdown can remove them cleanly.
    sockets: Mutex<Vec<PathBuf>>,
}

impl Default for CoreApiServer {
    fn default() -> Self {
        Self::new()
    }
}

impl CoreApiServer {
    pub fn new() -> Self {
        Self {
            sockets: Mutex::new(Vec::new()),
        }
    }

    /// Spawn a listener task for this plugin. Idempotent — if a socket for
    /// the plugin already exists it is removed and re-bound.
    pub async fn start(&self, manifest: PluginManifest, db: DbPool) -> Result<(), String> {
        let socket_path = super::core_api_socket_path(&manifest.id);
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create socket dir: {}", e))?;
        }
        // Remove stale socket from previous run.
        if socket_path.exists() {
            let _ = std::fs::remove_file(&socket_path);
        }

        let listener = UnixListener::bind(&socket_path)
            .map_err(|e| format!("bind {}: {}", socket_path.display(), e))?;
        // 0600 permissions — best-effort on Unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(md) = std::fs::metadata(&socket_path) {
                let mut perms = md.permissions();
                perms.set_mode(0o600);
                let _ = std::fs::set_permissions(&socket_path, perms);
            }
        }

        self.sockets.lock().await.push(socket_path.clone());

        let manifest = Arc::new(manifest);
        let db_pool = db;
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let manifest = manifest.clone();
                        let db = db_pool.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, manifest, db).await {
                                warn!("core-api connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        warn!("core-api accept error: {}", e);
                        break;
                    }
                }
            }
        });
        Ok(())
    }
}

async fn handle_connection(
    stream: UnixStream,
    manifest: Arc<PluginManifest>,
    db: DbPool,
) -> Result<(), String> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader).lines();
    while let Some(line) = reader
        .next_line()
        .await
        .map_err(|e| format!("read: {}", e))?
    {
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                debug!("bad JSON on core-api: {}", e);
                continue;
            }
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(Value::Null);

        let response = match dispatch(&manifest, &db, method, params).await {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(e) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32000, "message": e }
            }),
        };
        let mut frame = response.to_string();
        frame.push('\n');
        writer
            .write_all(frame.as_bytes())
            .await
            .map_err(|e| format!("write: {}", e))?;
    }
    Ok(())
}

async fn dispatch(
    manifest: &PluginManifest,
    db: &DbPool,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let plugin_id = manifest.id.as_str();
    match method {
        "entity.list" => {
            let entity_type = params
                .get("entityType")
                .and_then(Value::as_str)
                .ok_or_else(|| "entityType required".to_string())?;
            let filter = entities::ListFilter {
                project_id: params
                    .get("projectId")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                limit: params.get("limit").and_then(Value::as_i64),
                offset: params.get("offset").and_then(Value::as_i64),
            };
            Ok(json!(entities::list(db, plugin_id, entity_type, &filter)?))
        }
        "entity.get" => {
            let id = params
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "id required".to_string())?;
            Ok(json!(entities::get(db, id)?))
        }
        "entity.create" => {
            let entity_type = params
                .get("entityType")
                .and_then(Value::as_str)
                .ok_or_else(|| "entityType required".to_string())?;
            let data = params
                .get("data")
                .cloned()
                .ok_or_else(|| "data required".to_string())?;
            let project_id = params.get("projectId").and_then(Value::as_str);
            Ok(json!(entities::create(
                db, plugin_id, entity_type, project_id, &data, None
            )?))
        }
        "entity.update" => {
            let id = params
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "id required".to_string())?;
            let data = params
                .get("data")
                .cloned()
                .ok_or_else(|| "data required".to_string())?;
            Ok(json!(entities::update(db, id, &data)?))
        }
        "entity.delete" => {
            let id = params
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "id required".to_string())?;
            entities::delete(db, id)?;
            Ok(json!({ "deleted": id }))
        }
        "entity.link" => {
            let from_id = params
                .get("fromId")
                .and_then(Value::as_str)
                .ok_or_else(|| "fromId required".to_string())?;
            let from_type = params
                .get("fromType")
                .and_then(Value::as_str)
                .ok_or_else(|| "fromType required".to_string())?;
            let to_kind = params
                .get("toKind")
                .and_then(Value::as_str)
                .unwrap_or("plugin");
            let to_type = params
                .get("toType")
                .and_then(Value::as_str)
                .ok_or_else(|| "toType required".to_string())?;
            let to_id = params
                .get("toId")
                .and_then(Value::as_str)
                .ok_or_else(|| "toId required".to_string())?;
            let relation = params
                .get("relation")
                .and_then(Value::as_str)
                .ok_or_else(|| "relation required".to_string())?;
            Ok(json!(entities::link(
                db, "plugin", from_type, from_id, to_kind, to_type, to_id, relation
            )?))
        }
        "entity.unlink" => {
            let from_id = params
                .get("fromId")
                .and_then(Value::as_str)
                .ok_or_else(|| "fromId required".to_string())?;
            let to_id = params
                .get("toId")
                .and_then(Value::as_str)
                .ok_or_else(|| "toId required".to_string())?;
            let relation = params
                .get("relation")
                .and_then(Value::as_str)
                .ok_or_else(|| "relation required".to_string())?;
            entities::unlink(db, from_id, to_id, relation)?;
            Ok(json!({ "unlinked": true }))
        }
        "entity.list_relations" => {
            let id = params
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "id required".to_string())?;
            Ok(json!(entities::list_relations(db, id)?))
        }
        "work_item.get" => {
            if !manifest
                .permissions
                .core_entities
                .iter()
                .any(|e| e == "work_item")
            {
                return Err("plugin lacks core-entity `work_item` permission".into());
            }
            let id = params
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "id required".to_string())?;
            let conn = db.get().map_err(|e| e.to_string())?;
            let row = conn
                .query_row(
                    "SELECT id, project_id, title, status, created_at FROM work_items WHERE id = ?1",
                    rusqlite::params![id],
                    |row| {
                        Ok(json!({
                            "id": row.get::<_, String>(0)?,
                            "projectId": row.get::<_, String>(1)?,
                            "title": row.get::<_, String>(2)?,
                            "status": row.get::<_, String>(3)?,
                            "createdAt": row.get::<_, String>(4)?,
                        }))
                    },
                )
                .ok();
            Ok(row.unwrap_or(Value::Null))
        }
        "work_item.list" => {
            if !manifest
                .permissions
                .core_entities
                .iter()
                .any(|e| e == "work_item")
            {
                return Err("plugin lacks core-entity `work_item` permission".into());
            }
            let project_id = params
                .get("projectId")
                .and_then(Value::as_str)
                .ok_or_else(|| "projectId required".to_string())?;
            let conn = db.get().map_err(|e| e.to_string())?;
            let mut stmt = conn
                .prepare(
                    "SELECT id, project_id, title, status, created_at
                     FROM work_items WHERE project_id = ?1 ORDER BY created_at DESC LIMIT 200",
                )
                .map_err(|e| e.to_string())?;
            let items: Vec<Value> = stmt
                .query_map(rusqlite::params![project_id], |row| {
                    Ok(json!({
                        "id": row.get::<_, String>(0)?,
                        "projectId": row.get::<_, String>(1)?,
                        "title": row.get::<_, String>(2)?,
                        "status": row.get::<_, String>(3)?,
                        "createdAt": row.get::<_, String>(4)?,
                    }))
                })
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();
            Ok(json!({ "items": items }))
        }
        "workflow.fire_trigger" => {
            // Plugin-driven workflow trigger. Delivered to every enabled
            // workflow whose `trigger_kind` matches. Implementation punted
            // to the orchestrator slice; for V1 we ack and log the event so
            // developers can see it arrived.
            let kind = params
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            tracing::info!(plugin_id, kind = %kind, "workflow.fire_trigger received");
            Ok(json!({ "accepted": true }))
        }
        other => Err(format!("unknown method {:?}", other)),
    }
}
