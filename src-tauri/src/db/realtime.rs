//! Supabase Realtime WebSocket sync.
//!
//! Opens one WebSocket to `{base_url}/realtime/v1/websocket`, joins a single
//! channel with one `postgres_changes` subscription per synced table, and
//! dispatches INSERT/UPDATE/DELETE events to the local SQLite write helpers.
//!
//! Architecture:
//! - `RealtimeSyncState` holds the abort handle for the current sync task.
//! - `start_realtime_sync()` is the public entry point called from auth and startup.
//! - A reconnect loop with exponential back-off restores the connection after drops.
//! - After every successful channel join, `pull_all_data()` runs as a catch-up.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use serde_json::{json, Value};
use tauri::Emitter;
use tokio::sync::Mutex;
use tokio::task::AbortHandle;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, info, warn};

use crate::db::cloud::{
    write_agent_conversations, write_agents, write_bus_messages, write_bus_subscriptions,
    write_chat_compaction_summaries, write_chat_messages, write_chat_sessions,
    write_memory_extraction_log, write_project_agents, write_projects, write_runs,
    write_schedules, write_tasks, write_users, SupabaseClient,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const MAX_RECONNECT_DELAY_SECS: u64 = 60;
const INITIAL_RECONNECT_DELAY_SECS: u64 = 2;
const CHANNEL_TOPIC: &str = "realtime:db-sync";

/// All tables subscribed to via Realtime postgres_changes.
/// Must stay in sync with SYNCED_TABLES in workspace_sync and the migration.
const SYNCED_TABLES: &[&str] = &[
    "agents",
    "tasks",
    "schedules",
    "runs",
    "agent_conversations",
    "chat_sessions",
    "chat_messages",
    "chat_compaction_summaries",
    "bus_messages",
    "bus_subscriptions",
    "users",
    "memory_extraction_log",
    "projects",
    "project_agents",
    "workspace_objects",
];

// ---------------------------------------------------------------------------
// RealtimeSyncState — Tauri managed state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct RealtimeSyncState(pub Arc<Mutex<Option<AbortHandle>>>);

impl RealtimeSyncState {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }

    async fn replace(&self, handle: AbortHandle) {
        let mut guard = self.0.lock().await;
        if let Some(old) = guard.take() {
            old.abort();
        }
        *guard = Some(handle);
    }

    pub async fn stop(&self) {
        let mut guard = self.0.lock().await;
        if let Some(handle) = guard.take() {
            handle.abort();
            info!("Realtime sync task stopped");
        }
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub async fn start_realtime_sync(
    client: Arc<SupabaseClient>,
    pool: Pool<SqliteConnectionManager>,
    app: tauri::AppHandle,
    state: &RealtimeSyncState,
) {
    let jh = tokio::spawn(realtime_loop(client, pool, app));
    state.replace(jh.abort_handle()).await;
    info!("Realtime sync started");
}

// ---------------------------------------------------------------------------
// Reconnect loop with exponential back-off
// ---------------------------------------------------------------------------

async fn realtime_loop(
    client: Arc<SupabaseClient>,
    pool: Pool<SqliteConnectionManager>,
    app: tauri::AppHandle,
) {
    let mut delay_secs = INITIAL_RECONNECT_DELAY_SECS;
    loop {
        match connect_and_listen(&client, &pool, &app).await {
            Ok(_) => {
                info!("Realtime WS closed cleanly — reconnecting");
                delay_secs = INITIAL_RECONNECT_DELAY_SECS;
            }
            Err(e) => {
                warn!("Realtime WS error: {} — reconnecting in {}s", e, delay_secs);
            }
        }
        tokio::time::sleep(Duration::from_secs(delay_secs)).await;
        delay_secs = (delay_secs * 2).min(MAX_RECONNECT_DELAY_SECS);
    }
}

// ---------------------------------------------------------------------------
// Single WebSocket session
// ---------------------------------------------------------------------------

async fn connect_and_listen(
    client: &Arc<SupabaseClient>,
    pool: &Pool<SqliteConnectionManager>,
    app: &tauri::AppHandle,
) -> Result<(), String> {
    let url = ws_url(client.base_url(), client.anon_key());
    let token = client.fresh_token().await?;

    let (ws_stream, _) = connect_async(url)
        .await
        .map_err(|e| format!("WS connect: {e}"))?;

    let (mut write, mut read) = ws_stream.split();

    // Send join message
    let join = build_join_msg(&token, &client.user_id);
    write
        .send(Message::Text(join.to_string()))
        .await
        .map_err(|e| format!("WS join send: {e}"))?;

    info!("Realtime WS connected, join sent");

    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.tick().await; // skip the immediate first tick

    let mut joined = false;

    loop {
        tokio::select! {
            msg = read.next() => {
                match msg {
                    None | Some(Ok(Message::Close(_))) => return Ok(()),
                    Some(Err(e)) => return Err(format!("WS recv: {e}")),
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(v) = serde_json::from_str::<Value>(&text) {
                            if !joined {
                                joined = maybe_confirm_join(&v, client, pool);
                            } else {
                                handle_message(&v, pool, app, client);
                            }
                        }
                    }
                    Some(Ok(_)) => {} // ignore binary/ping/pong frames
                }
            }
            _ = heartbeat.tick() => {
                let hb = json!({
                    "topic": "phoenix",
                    "event": "heartbeat",
                    "payload": {},
                    "ref": "heartbeat"
                });
                if let Err(e) = write.send(Message::Text(hb.to_string())).await {
                    return Err(format!("WS heartbeat: {e}"));
                }
            }
        }
    }
}

/// Returns true if the phx_reply confirms a successful join; triggers catch-up pull.
fn maybe_confirm_join(
    v: &Value,
    client: &Arc<SupabaseClient>,
    pool: &Pool<SqliteConnectionManager>,
) -> bool {
    let event = v.get("event").and_then(|e| e.as_str()).unwrap_or("");
    if event != "phx_reply" {
        return false;
    }
    let status = v
        .pointer("/payload/status")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    if status == "ok" {
        info!("Realtime channel joined — triggering catch-up pull");
        let c = client.clone();
        let p = pool.clone();
        tokio::spawn(async move {
            if let Err(e) = c.pull_all_data(&p).await {
                warn!("Realtime catch-up pull failed: {e}");
            }
        });
        true
    } else {
        warn!("Realtime join rejected (status={})", status);
        false
    }
}

// ---------------------------------------------------------------------------
// Message dispatch
// ---------------------------------------------------------------------------

fn handle_message(
    v: &Value,
    pool: &Pool<SqliteConnectionManager>,
    app: &tauri::AppHandle,
    client: &Arc<SupabaseClient>,
) {
    let event = v.get("event").and_then(|e| e.as_str()).unwrap_or("");
    if event != "postgres_changes" {
        return;
    }
    let data = match v.pointer("/payload/data") {
        Some(d) if d.is_object() => d.clone(),
        _ => return,
    };
    let change_type = data
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_owned();
    let table = data
        .get("table")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_owned();

    if table.is_empty() || change_type.is_empty() {
        return;
    }

    debug!("Realtime event: {} on {}", change_type, table);

    let pool = pool.clone();
    let app = app.clone();
    let client = client.clone();

    // For workspace_objects INSERT/UPDATE, also trigger async file sync
    if table == "workspace_objects" && (change_type == "INSERT" || change_type == "UPDATE") {
        if let Some(record) = data.get("record").cloned() {
            let pool_ws = pool.clone();
            let client_ws = client.clone();
            tokio::spawn(async move {
                crate::db::workspace_sync::apply_remote_workspace_change(
                    &client_ws,
                    &pool_ws,
                    &record,
                )
                .await;
            });
        }
    }

    tokio::task::spawn_blocking(move || {
        if let Err(e) = apply_change(&table, &change_type, &data, &pool) {
            warn!(
                "Realtime apply_change failed ({} {}): {}",
                change_type, table, e
            );
        } else {
            emit_sync_event(&app, &table, &data, &change_type);
        }
    });
}

fn apply_change(
    table: &str,
    change_type: &str,
    data: &Value,
    pool: &Pool<SqliteConnectionManager>,
) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;

    match change_type {
        "INSERT" | "UPDATE" => {
            let record = match data.get("record") {
                Some(r) if r.is_object() => r.clone(),
                _ => return Ok(()),
            };
            dispatch_write(table, vec![record], &conn)
        }
        "DELETE" => {
            let old = match data.get("old_record") {
                Some(r) if r.is_object() => r.clone(),
                _ => return Ok(()),
            };
            dispatch_delete(table, &old, &conn)
        }
        _ => Ok(()),
    }
}

fn dispatch_write(
    table: &str,
    rows: Vec<Value>,
    conn: &rusqlite::Connection,
) -> Result<(), String> {
    match table {
        "agents" => write_agents(conn, rows),
        "tasks" => write_tasks(conn, rows),
        "schedules" => write_schedules(conn, rows),
        "runs" => write_runs(conn, rows),
        "agent_conversations" => write_agent_conversations(conn, rows),
        "chat_sessions" => write_chat_sessions(conn, rows),
        "chat_messages" => write_chat_messages(conn, rows),
        "chat_compaction_summaries" => write_chat_compaction_summaries(conn, rows),
        "bus_messages" => write_bus_messages(conn, rows),
        "bus_subscriptions" => write_bus_subscriptions(conn, rows),
        "users" => write_users(conn, rows),
        "memory_extraction_log" => write_memory_extraction_log(conn, rows),
        "projects" => write_projects(conn, rows),
        "project_agents" => write_project_agents(conn, rows),
        "workspace_objects" => write_workspace_objects(conn, rows),
        other => {
            warn!("Realtime: no write handler for table '{}'", other);
            Ok(())
        }
    }
}

fn dispatch_delete(
    table: &str,
    old: &Value,
    conn: &rusqlite::Connection,
) -> Result<(), String> {
    // Validate table against allowlist before interpolating into SQL.
    if !SYNCED_TABLES.contains(&table) {
        return Err(format!("Realtime DELETE: unknown table '{}'", table));
    }
    match table {
        "project_agents" => {
            // Composite PK: (project_id, agent_id)
            let project_id = old.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            let agent_id = old.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
            if project_id.is_empty() || agent_id.is_empty() {
                return Ok(());
            }
            conn.execute(
                "DELETE FROM project_agents WHERE project_id = ?1 AND agent_id = ?2",
                rusqlite::params![project_id, agent_id],
            )
            .map_err(|e| e.to_string())?;
        }
        "workspace_objects" => {
            // workspace_objects uses soft-deletes (deleted_at tombstones).
            // A hard-DELETE event shouldn't occur, but handle it gracefully.
            let user_id = old.get("user_id").and_then(|v| v.as_str()).unwrap_or("");
            let scope_type = old.get("scope_type").and_then(|v| v.as_str()).unwrap_or("");
            let scope_id = old.get("scope_id").and_then(|v| v.as_str()).unwrap_or("");
            let path = old.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if user_id.is_empty() || scope_type.is_empty() || scope_id.is_empty() || path.is_empty()
            {
                return Ok(());
            }
            conn.execute(
                "DELETE FROM workspace_objects
                 WHERE user_id=?1 AND scope_type=?2 AND scope_id=?3 AND path=?4",
                rusqlite::params![user_id, scope_type, scope_id, path],
            )
            .map_err(|e| e.to_string())?;
        }
        _ => {
            // Single `id` PK — safe because table is validated against SYNCED_TABLES above.
            let id = old.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if id.is_empty() {
                return Ok(());
            }
            let sql = format!("DELETE FROM {table} WHERE id = ?1");
            conn.execute(&sql, rusqlite::params![id])
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// workspace_objects — local SQLite manifest
// ---------------------------------------------------------------------------

fn write_workspace_objects(
    conn: &rusqlite::Connection,
    rows: Vec<Value>,
) -> Result<(), String> {
    for r in rows {
        let user_id = r.get("user_id").and_then(|v| v.as_str()).unwrap_or("");
        let scope_type = r.get("scope_type").and_then(|v| v.as_str()).unwrap_or("");
        let scope_id = r.get("scope_id").and_then(|v| v.as_str()).unwrap_or("");
        let path = r.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let storage_path = r.get("storage_path").and_then(|v| v.as_str()).unwrap_or("");
        let sha256 = r.get("sha256").and_then(|v| v.as_str()).unwrap_or("");
        let size_bytes = r.get("size_bytes").and_then(|v| v.as_i64()).unwrap_or(0);
        let mime_type = r.get("mime_type").and_then(|v| v.as_str());
        let version = r.get("version").and_then(|v| v.as_i64()).unwrap_or(0);
        let deleted_at = r.get("deleted_at").and_then(|v| v.as_str());
        let updated_at = r.get("updated_at").and_then(|v| v.as_str()).unwrap_or("");

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
                mime_type,
                version,
                deleted_at,
                updated_at,
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ws_url(base_url: &str, anon_key: &str) -> String {
    let host = base_url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    format!(
        "wss://{}/realtime/v1/websocket?apikey={}&vsn=1.0.0",
        host, anon_key
    )
}

fn build_join_msg(token: &str, user_id: &str) -> Value {
    let changes: Vec<Value> = SYNCED_TABLES
        .iter()
        .map(|table| {
            json!({
                "event": "*",
                "schema": "public",
                "table": table,
                "filter": format!("user_id=eq.{}", user_id)
            })
        })
        .collect();

    json!({
        "topic": CHANNEL_TOPIC,
        "event": "phx_join",
        "payload": {
            "access_token": token,
            "config": {
                "postgres_changes": changes
            }
        },
        "ref": "1"
    })
}

fn emit_sync_event(app: &tauri::AppHandle, table: &str, data: &Value, change_type: &str) {
    let record = if change_type == "DELETE" {
        data.get("old_record")
    } else {
        data.get("record")
    };

    // For workspace_objects include scope info so the frontend can invalidate
    // the right query key without fetching everything.
    let mut payload = json!({ "table": table });
    if table == "workspace_objects" {
        if let Some(r) = record {
            if let Some(obj) = payload.as_object_mut() {
                if let Some(st) = r.get("scope_type") {
                    obj.insert("scope_type".into(), st.clone());
                }
                if let Some(si) = r.get("scope_id") {
                    obj.insert("scope_id".into(), si.clone());
                }
            }
        }
    }

    if let Err(e) = app.emit("sync:remote_change", payload) {
        debug!("Could not emit sync:remote_change: {e}");
    }
}
