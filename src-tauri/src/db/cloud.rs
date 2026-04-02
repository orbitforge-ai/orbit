//! Supabase cloud sync layer.
//!
//! Architecture: local SQLite is always the primary read/write store.
//! In cloud mode every mutating command also fires a background upsert to
//! Supabase (PostgREST REST API over reqwest).  On login the user's cloud
//! data is merged into local SQLite so a new device starts with their history.

use std::sync::Arc;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use serde_json::Value;
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// SupabaseClient
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct SupabaseClient {
    http: reqwest::Client,
    /// https://yourproject.supabase.co  (no trailing slash)
    base_url: String,
    anon_key: String,
    /// JWT — wrapped in Arc so the token can be refreshed in place.
    access_token: Arc<std::sync::RwLock<String>>,
    pub user_id: String,
}

impl SupabaseClient {
    pub fn new(
        base_url: String,
        anon_key: String,
        access_token: String,
        user_id: String,
    ) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url,
            anon_key,
            access_token: Arc::new(std::sync::RwLock::new(access_token)),
            user_id,
        }
    }

    fn token(&self) -> String {
        self.access_token.read().unwrap().clone()
    }

    // -----------------------------------------------------------------------
    // Generic REST helpers
    // -----------------------------------------------------------------------

    /// GET all rows from a table (RLS filters to current user automatically).
    async fn get_table(&self, table: &str) -> Result<Vec<Value>, String> {
        let url = format!(
            "{}/rest/v1/{}?order=created_at.asc&limit=10000",
            self.base_url, table
        );
        let resp = self
            .http
            .get(&url)
            .header("apikey", &self.anon_key)
            .header("Authorization", format!("Bearer {}", self.token()))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| format!("GET {table}: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("GET {table} {status}: {text}"));
        }
        resp.json::<Vec<Value>>()
            .await
            .map_err(|e| format!("parse {table}: {e}"))
    }

    /// POST a single row with UPSERT semantics (merge on PK conflict).
    pub async fn upsert_single(&self, table: &str, body: Value) -> Result<(), String> {
        self.upsert_batch(table, vec![body]).await
    }

    /// POST an array of rows with UPSERT semantics.
    pub async fn upsert_batch(&self, table: &str, rows: Vec<Value>) -> Result<(), String> {
        if rows.is_empty() {
            return Ok(());
        }
        let url = format!("{}/rest/v1/{}", self.base_url, table);
        let resp = self
            .http
            .post(&url)
            .header("apikey", &self.anon_key)
            .header("Authorization", format!("Bearer {}", self.token()))
            .header("Content-Type", "application/json")
            .header("Prefer", "resolution=merge-duplicates")
            .json(&rows)
            .send()
            .await
            .map_err(|e| format!("POST {table}: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("UPSERT {table} {status}: {text}"));
        }
        Ok(())
    }

    /// PATCH a row by its `id` column (partial update).
    pub async fn patch_by_id(&self, table: &str, id: &str, updates: Value) -> Result<(), String> {
        let url = format!("{}/rest/v1/{}?id=eq.{}", self.base_url, table, id);
        let resp = self
            .http
            .patch(&url)
            .header("apikey", &self.anon_key)
            .header("Authorization", format!("Bearer {}", self.token()))
            .header("Content-Type", "application/json")
            .json(&updates)
            .send()
            .await
            .map_err(|e| format!("PATCH {table} {id}: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("PATCH {table} {id} {status}: {text}"));
        }
        Ok(())
    }

    /// DELETE a row by its `id` column.  RLS protects against cross-user deletes.
    pub async fn delete_by_id(&self, table: &str, id: &str) -> Result<(), String> {
        let url = format!("{}/rest/v1/{}?id=eq.{}", self.base_url, table, id);
        let resp = self
            .http
            .delete(&url)
            .header("apikey", &self.anon_key)
            .header("Authorization", format!("Bearer {}", self.token()))
            .send()
            .await
            .map_err(|e| format!("DELETE {table} {id}: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("DELETE {table} {id} {status}: {text}"));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Entity-specific upsert helpers (build snake_case JSON bodies)
    // Note: all model structs use #[serde(rename_all = "camelCase")], so we
    // cannot use serde_json::to_value() directly — Supabase expects snake_case.
    // -----------------------------------------------------------------------

    pub async fn upsert_agent(&self, a: &crate::models::agent::Agent) -> Result<(), String> {
        self.upsert_single(
            "agents",
            serde_json::json!({
                "user_id": self.user_id,
                "id": a.id,
                "name": a.name,
                "description": a.description,
                "state": a.state,
                "max_concurrent_runs": a.max_concurrent_runs,
                "heartbeat_at": a.heartbeat_at,
                "model_config": "{}",
                "created_at": a.created_at,
                "updated_at": a.updated_at,
            }),
        )
        .await
    }

    pub async fn upsert_task(&self, t: &crate::models::task::Task) -> Result<(), String> {
        self.upsert_single(
            "tasks",
            serde_json::json!({
                "user_id": self.user_id,
                "id": t.id,
                "name": t.name,
                "description": t.description,
                "kind": t.kind,
                "config": t.config,
                "max_duration_seconds": t.max_duration_seconds,
                "max_retries": t.max_retries,
                "retry_delay_seconds": t.retry_delay_seconds,
                "concurrency_policy": t.concurrency_policy,
                "tags": t.tags,
                "agent_id": t.agent_id,
                "enabled": t.enabled,
                "created_at": t.created_at,
                "updated_at": t.updated_at,
            }),
        )
        .await
    }

    pub async fn upsert_schedule(
        &self,
        s: &crate::models::schedule::Schedule,
    ) -> Result<(), String> {
        self.upsert_single(
            "schedules",
            serde_json::json!({
                "user_id": self.user_id,
                "id": s.id,
                "task_id": s.task_id,
                "kind": s.kind,
                "config": s.config,
                "enabled": s.enabled,
                "next_run_at": s.next_run_at,
                "last_run_at": s.last_run_at,
                "created_at": s.created_at,
                "updated_at": s.updated_at,
            }),
        )
        .await
    }

    pub async fn upsert_chat_session(
        &self,
        cs: &crate::models::chat::ChatSession,
    ) -> Result<(), String> {
        // Exclude join-derived fields (source_agent_id, source_agent_name, etc.)
        self.upsert_single(
            "chat_sessions",
            serde_json::json!({
                "user_id": self.user_id,
                "id": cs.id,
                "agent_id": cs.agent_id,
                "title": cs.title,
                "archived": cs.archived,
                "session_type": cs.session_type,
                "parent_session_id": cs.parent_session_id,
                "source_bus_message_id": cs.source_bus_message_id,
                "chain_depth": cs.chain_depth,
                "execution_state": cs.execution_state,
                "finish_summary": cs.finish_summary,
                "terminal_error": cs.terminal_error,
                "created_at": cs.created_at,
                "updated_at": cs.updated_at,
            }),
        )
        .await
    }

    pub async fn upsert_bus_subscription(
        &self,
        s: &crate::models::bus::BusSubscription,
    ) -> Result<(), String> {
        self.upsert_single(
            "bus_subscriptions",
            serde_json::json!({
                "user_id": self.user_id,
                "id": s.id,
                "subscriber_agent_id": s.subscriber_agent_id,
                "source_agent_id": s.source_agent_id,
                "event_type": s.event_type,
                "task_id": s.task_id,
                "payload_template": s.payload_template,
                "enabled": s.enabled,
                "max_chain_depth": s.max_chain_depth,
                "created_at": s.created_at,
                "updated_at": s.updated_at,
            }),
        )
        .await
    }

    pub async fn upsert_user(&self, u: &crate::models::user::User) -> Result<(), String> {
        self.upsert_single(
            "users",
            serde_json::json!({
                "user_id": self.user_id,
                "id": u.id,
                "name": u.name,
                "is_default": u.is_default,
                "created_at": u.created_at,
            }),
        )
        .await
    }

    // -----------------------------------------------------------------------
    // Full bi-directional sync (called on login)
    // -----------------------------------------------------------------------

    /// Push all local SQLite data to Supabase (local wins on conflict).
    /// Called BEFORE pull so cloud gets any offline changes made on this device.
    pub async fn push_local_data(
        &self,
        pool: &Pool<SqliteConnectionManager>,
    ) -> Result<(), String> {
        let user_id = self.user_id.clone();
        let p = pool.clone();

        let rows = tokio::task::spawn_blocking(move || {
            let conn = p.get().map_err(|e| e.to_string())?;
            Ok::<_, String>((
                read_agents(&conn, &user_id)?,
                read_tasks(&conn, &user_id)?,
                read_schedules(&conn, &user_id)?,
                read_runs(&conn, &user_id)?,
                read_agent_conversations(&conn, &user_id)?,
                read_chat_sessions(&conn, &user_id)?,
                read_chat_messages(&conn, &user_id)?,
                read_chat_compaction_summaries(&conn, &user_id)?,
                read_bus_messages(&conn, &user_id)?,
                read_bus_subscriptions(&conn, &user_id)?,
                read_users(&conn, &user_id)?,
                read_memory_extraction_log(&conn, &user_id)?,
            ))
        })
        .await
        .map_err(|e| e.to_string())??;

        let (agents, tasks, scheds, runs, convos, sessions, msgs, summaries, bus_msgs, bus_subs, users, mem_log) = rows;

        // Batch upsert each table; log failures but don't abort
        macro_rules! push {
            ($table:expr, $data:expr) => {
                if let Err(e) = self.upsert_batch($table, $data).await {
                    warn!("push {} failed: {}", $table, e);
                }
            };
        }
        push!("agents", agents);
        push!("tasks", tasks);
        push!("schedules", scheds);
        push!("runs", runs);
        push!("agent_conversations", convos);
        push!("chat_sessions", sessions);
        push!("chat_messages", msgs);
        push!("chat_compaction_summaries", summaries);
        push!("bus_messages", bus_msgs);
        push!("bus_subscriptions", bus_subs);
        push!("users", users);
        push!("memory_extraction_log", mem_log);

        info!("Pushed local data to Supabase");
        Ok(())
    }

    /// Pull all cloud data into local SQLite (cloud wins on conflict for matching IDs).
    /// Called AFTER push so the device receives data from other devices.
    pub async fn pull_all_data(
        &self,
        pool: &Pool<SqliteConnectionManager>,
    ) -> Result<(), String> {
        macro_rules! fetch {
            ($table:expr) => {
                self.get_table($table).await.unwrap_or_else(|e| {
                    warn!("pull {} failed: {}", $table, e);
                    vec![]
                })
            };
        }

        let agents = fetch!("agents");
        let tasks = fetch!("tasks");
        let scheds = fetch!("schedules");
        let runs = fetch!("runs");
        let convos = fetch!("agent_conversations");
        let sessions = fetch!("chat_sessions");
        let msgs = fetch!("chat_messages");
        let summaries = fetch!("chat_compaction_summaries");
        let bus_msgs = fetch!("bus_messages");
        let bus_subs = fetch!("bus_subscriptions");
        let users = fetch!("users");
        let mem_log = fetch!("memory_extraction_log");

        let p = pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = p.get().map_err(|e| e.to_string())?;
            write_agents(&conn, agents)?;
            write_tasks(&conn, tasks)?;
            write_schedules(&conn, scheds)?;
            write_runs(&conn, runs)?;
            write_agent_conversations(&conn, convos)?;
            write_chat_sessions(&conn, sessions)?;
            write_chat_messages(&conn, msgs)?;
            write_chat_compaction_summaries(&conn, summaries)?;
            write_bus_messages(&conn, bus_msgs)?;
            write_bus_subscriptions(&conn, bus_subs)?;
            write_users(&conn, users)?;
            write_memory_extraction_log(&conn, mem_log)?;
            Ok::<(), String>(())
        })
        .await
        .map_err(|e| e.to_string())??;

        info!("Pulled cloud data into local SQLite");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CloudClientState — Tauri managed state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct CloudClientState(pub Arc<std::sync::RwLock<Option<Arc<SupabaseClient>>>>);

impl CloudClientState {
    pub fn empty() -> Self {
        Self(Arc::new(std::sync::RwLock::new(None)))
    }

    /// Returns a clone of the current client (if any).  Fast — just clones an Arc.
    pub fn get(&self) -> Option<Arc<SupabaseClient>> {
        self.0.read().unwrap().clone()
    }

    pub fn set(&self, client: Option<Arc<SupabaseClient>>) {
        *self.0.write().unwrap() = client;
    }
}

// ---------------------------------------------------------------------------
// SQLite → Supabase (push): read rows and build snake_case JSON bodies
// ---------------------------------------------------------------------------

fn json_or_null(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or(Value::Null)
}

fn read_agents(
    conn: &rusqlite::Connection,
    user_id: &str,
) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, description, state, max_concurrent_runs, heartbeat_at,
                    model_config, created_at, updated_at FROM agents",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let model_config: String = row.get(6)?;
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "description": row.get::<_, Option<String>>(2)?,
                "state": row.get::<_, String>(3)?,
                "max_concurrent_runs": row.get::<_, i64>(4)?,
                "heartbeat_at": row.get::<_, Option<String>>(5)?,
                "model_config": model_config,
                "created_at": row.get::<_, String>(7)?,
                "updated_at": row.get::<_, String>(8)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn read_tasks(conn: &rusqlite::Connection, user_id: &str) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, description, kind, config, max_duration_seconds, max_retries,
                    retry_delay_seconds, concurrency_policy, tags, agent_id, enabled,
                    created_at, updated_at FROM tasks",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let config_str: String = row.get(4)?;
            let tags_str: String = row.get(9)?;
            let enabled: bool = row.get(11)?;
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "description": row.get::<_, Option<String>>(2)?,
                "kind": row.get::<_, String>(3)?,
                "config": json_or_null(&config_str),
                "max_duration_seconds": row.get::<_, i64>(5)?,
                "max_retries": row.get::<_, i64>(6)?,
                "retry_delay_seconds": row.get::<_, i64>(7)?,
                "concurrency_policy": row.get::<_, String>(8)?,
                "tags": json_or_null(&tags_str),
                "agent_id": row.get::<_, Option<String>>(10)?,
                "enabled": enabled,
                "created_at": row.get::<_, String>(12)?,
                "updated_at": row.get::<_, String>(13)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn read_schedules(conn: &rusqlite::Connection, user_id: &str) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, task_id, kind, config, enabled, next_run_at, last_run_at,
                    created_at, updated_at FROM schedules",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let config_str: String = row.get(3)?;
            let enabled: bool = row.get(4)?;
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "task_id": row.get::<_, String>(1)?,
                "kind": row.get::<_, String>(2)?,
                "config": json_or_null(&config_str),
                "enabled": enabled,
                "next_run_at": row.get::<_, Option<String>>(5)?,
                "last_run_at": row.get::<_, Option<String>>(6)?,
                "created_at": row.get::<_, String>(7)?,
                "updated_at": row.get::<_, String>(8)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn read_runs(conn: &rusqlite::Connection, user_id: &str) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, task_id, schedule_id, agent_id, state, trigger, exit_code, pid,
                    log_path, started_at, finished_at, duration_ms, retry_count,
                    parent_run_id, metadata, chain_depth, source_bus_message_id,
                    is_sub_agent, created_at FROM runs",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let meta_str: String = row.get(14)?;
            let is_sub_agent: bool = row.get(17)?;
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "task_id": row.get::<_, String>(1)?,
                "schedule_id": row.get::<_, Option<String>>(2)?,
                "agent_id": row.get::<_, Option<String>>(3)?,
                "state": row.get::<_, String>(4)?,
                "trigger": row.get::<_, String>(5)?,
                "exit_code": row.get::<_, Option<i64>>(6)?,
                "pid": row.get::<_, Option<i64>>(7)?,
                "log_path": row.get::<_, String>(8)?,
                "started_at": row.get::<_, Option<String>>(9)?,
                "finished_at": row.get::<_, Option<String>>(10)?,
                "duration_ms": row.get::<_, Option<i64>>(11)?,
                "retry_count": row.get::<_, i64>(12)?,
                "parent_run_id": row.get::<_, Option<String>>(13)?,
                "metadata": json_or_null(&meta_str),
                "chain_depth": row.get::<_, i64>(15)?,
                "source_bus_message_id": row.get::<_, Option<String>>(16)?,
                "is_sub_agent": is_sub_agent,
                "created_at": row.get::<_, String>(18)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn read_agent_conversations(
    conn: &rusqlite::Connection,
    user_id: &str,
) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, agent_id, run_id, messages, total_input_tokens,
                    total_output_tokens, iterations, created_at, updated_at
             FROM agent_conversations",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let msgs_str: String = row.get(3)?;
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "agent_id": row.get::<_, String>(1)?,
                "run_id": row.get::<_, String>(2)?,
                "messages": json_or_null(&msgs_str),
                "total_input_tokens": row.get::<_, i64>(4)?,
                "total_output_tokens": row.get::<_, i64>(5)?,
                "iterations": row.get::<_, i64>(6)?,
                "created_at": row.get::<_, String>(7)?,
                "updated_at": row.get::<_, String>(8)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn read_chat_sessions(conn: &rusqlite::Connection, user_id: &str) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, agent_id, title, archived, last_input_tokens, session_type,
                    parent_session_id, source_bus_message_id, chain_depth,
                    execution_state, finish_summary, terminal_error,
                    created_at, updated_at FROM chat_sessions",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let archived: bool = row.get(3)?;
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "agent_id": row.get::<_, String>(1)?,
                "title": row.get::<_, String>(2)?,
                "archived": archived,
                "last_input_tokens": row.get::<_, Option<i64>>(4)?,
                "session_type": row.get::<_, String>(5)?,
                "parent_session_id": row.get::<_, Option<String>>(6)?,
                "source_bus_message_id": row.get::<_, Option<String>>(7)?,
                "chain_depth": row.get::<_, i64>(8)?,
                "execution_state": row.get::<_, Option<String>>(9)?,
                "finish_summary": row.get::<_, Option<String>>(10)?,
                "terminal_error": row.get::<_, Option<String>>(11)?,
                "created_at": row.get::<_, String>(12)?,
                "updated_at": row.get::<_, String>(13)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn read_chat_messages(conn: &rusqlite::Connection, user_id: &str) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, role, content, token_count, is_compacted, created_at
             FROM chat_messages",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let is_compacted: bool = row.get(5)?;
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "session_id": row.get::<_, String>(1)?,
                "role": row.get::<_, String>(2)?,
                "content": row.get::<_, String>(3)?,
                "token_count": row.get::<_, Option<i64>>(4)?,
                "is_compacted": is_compacted,
                "created_at": row.get::<_, String>(6)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn read_chat_compaction_summaries(
    conn: &rusqlite::Connection,
    user_id: &str,
) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, summary_message_id, compacted_message_ids,
                    original_token_count, summary_token_count, created_at
             FROM chat_compaction_summaries",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let ids_str: String = row.get(3)?;
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "session_id": row.get::<_, String>(1)?,
                "summary_message_id": row.get::<_, String>(2)?,
                "compacted_message_ids": json_or_null(&ids_str),
                "original_token_count": row.get::<_, i64>(4)?,
                "summary_token_count": row.get::<_, i64>(5)?,
                "created_at": row.get::<_, String>(6)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn read_bus_messages(conn: &rusqlite::Connection, user_id: &str) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, from_agent_id, from_run_id, from_session_id,
                    to_agent_id, to_run_id, to_session_id,
                    kind, event_type, payload, status, created_at
             FROM bus_messages",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let payload_str: String = row.get(9)?;
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "from_agent_id": row.get::<_, String>(1)?,
                "from_run_id": row.get::<_, Option<String>>(2)?,
                "from_session_id": row.get::<_, Option<String>>(3)?,
                "to_agent_id": row.get::<_, String>(4)?,
                "to_run_id": row.get::<_, Option<String>>(5)?,
                "to_session_id": row.get::<_, Option<String>>(6)?,
                "kind": row.get::<_, String>(7)?,
                "event_type": row.get::<_, Option<String>>(8)?,
                "payload": json_or_null(&payload_str),
                "status": row.get::<_, String>(10)?,
                "created_at": row.get::<_, String>(11)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn read_bus_subscriptions(
    conn: &rusqlite::Connection,
    user_id: &str,
) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, subscriber_agent_id, source_agent_id, event_type, task_id,
                    payload_template, enabled, max_chain_depth, created_at, updated_at
             FROM bus_subscriptions",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let enabled: bool = row.get(6)?;
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "subscriber_agent_id": row.get::<_, String>(1)?,
                "source_agent_id": row.get::<_, String>(2)?,
                "event_type": row.get::<_, String>(3)?,
                "task_id": row.get::<_, String>(4)?,
                "payload_template": row.get::<_, String>(5)?,
                "enabled": enabled,
                "max_chain_depth": row.get::<_, i64>(7)?,
                "created_at": row.get::<_, String>(8)?,
                "updated_at": row.get::<_, String>(9)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn read_users(conn: &rusqlite::Connection, user_id: &str) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare("SELECT id, name, is_default, created_at FROM users")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let is_default: bool = row.get(2)?;
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "is_default": is_default,
                "created_at": row.get::<_, String>(3)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn read_memory_extraction_log(
    conn: &rusqlite::Connection,
    user_id: &str,
) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, agent_id, memories_extracted, status, created_at
             FROM memory_extraction_log",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "session_id": row.get::<_, Option<String>>(1)?,
                "agent_id": row.get::<_, Option<String>>(2)?,
                "memories_extracted": row.get::<_, i64>(3)?,
                "status": row.get::<_, String>(4)?,
                "created_at": row.get::<_, String>(5)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Supabase → SQLite (pull): write cloud rows into local tables
// ---------------------------------------------------------------------------

fn str_val(row: &Value, key: &str) -> String {
    row[key].as_str().unwrap_or("").to_string()
}

fn opt_str(row: &Value, key: &str) -> Option<String> {
    row[key].as_str().map(String::from)
}

fn int_val(row: &Value, key: &str, default: i64) -> i64 {
    row[key].as_i64().unwrap_or(default)
}

fn bool_val(row: &Value, key: &str) -> i64 {
    if row[key].as_bool().unwrap_or(false) { 1 } else { 0 }
}

fn json_str(row: &Value, key: &str, default: &str) -> String {
    if row[key].is_null() || row[key].is_object() || row[key].is_array() {
        serde_json::to_string(&row[key]).unwrap_or_else(|_| default.to_string())
    } else {
        row[key].as_str().unwrap_or(default).to_string()
    }
}

fn write_agents(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO agents
             (id, name, description, state, max_concurrent_runs, heartbeat_at,
              model_config, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            rusqlite::params![
                str_val(&r, "id"),
                str_val(&r, "name"),
                opt_str(&r, "description"),
                str_val(&r, "state"),
                int_val(&r, "max_concurrent_runs", 5),
                opt_str(&r, "heartbeat_at"),
                str_val(&r, "model_config"),
                str_val(&r, "created_at"),
                str_val(&r, "updated_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_tasks(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO tasks
             (id, name, description, kind, config, max_duration_seconds, max_retries,
              retry_delay_seconds, concurrency_policy, tags, agent_id, enabled,
              created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            rusqlite::params![
                str_val(&r, "id"),
                str_val(&r, "name"),
                opt_str(&r, "description"),
                str_val(&r, "kind"),
                json_str(&r, "config", "{}"),
                int_val(&r, "max_duration_seconds", 3600),
                int_val(&r, "max_retries", 0),
                int_val(&r, "retry_delay_seconds", 60),
                str_val(&r, "concurrency_policy"),
                json_str(&r, "tags", "[]"),
                opt_str(&r, "agent_id"),
                bool_val(&r, "enabled"),
                str_val(&r, "created_at"),
                str_val(&r, "updated_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_schedules(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO schedules
             (id, task_id, kind, config, enabled, next_run_at, last_run_at,
              created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            rusqlite::params![
                str_val(&r, "id"),
                str_val(&r, "task_id"),
                str_val(&r, "kind"),
                json_str(&r, "config", "{}"),
                bool_val(&r, "enabled"),
                opt_str(&r, "next_run_at"),
                opt_str(&r, "last_run_at"),
                str_val(&r, "created_at"),
                str_val(&r, "updated_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_runs(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO runs
             (id, task_id, schedule_id, agent_id, state, trigger, exit_code, pid,
              log_path, started_at, finished_at, duration_ms, retry_count,
              parent_run_id, metadata, chain_depth, source_bus_message_id,
              is_sub_agent, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
            rusqlite::params![
                str_val(&r, "id"),
                str_val(&r, "task_id"),
                opt_str(&r, "schedule_id"),
                opt_str(&r, "agent_id"),
                str_val(&r, "state"),
                str_val(&r, "trigger"),
                r["exit_code"].as_i64(),
                r["pid"].as_i64(),
                str_val(&r, "log_path"),
                opt_str(&r, "started_at"),
                opt_str(&r, "finished_at"),
                r["duration_ms"].as_i64(),
                int_val(&r, "retry_count", 0),
                opt_str(&r, "parent_run_id"),
                json_str(&r, "metadata", "{}"),
                int_val(&r, "chain_depth", 0),
                opt_str(&r, "source_bus_message_id"),
                bool_val(&r, "is_sub_agent"),
                str_val(&r, "created_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_agent_conversations(
    conn: &rusqlite::Connection,
    rows: Vec<Value>,
) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO agent_conversations
             (id, agent_id, run_id, messages, total_input_tokens,
              total_output_tokens, iterations, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            rusqlite::params![
                str_val(&r, "id"),
                str_val(&r, "agent_id"),
                str_val(&r, "run_id"),
                json_str(&r, "messages", "[]"),
                int_val(&r, "total_input_tokens", 0),
                int_val(&r, "total_output_tokens", 0),
                int_val(&r, "iterations", 0),
                str_val(&r, "created_at"),
                str_val(&r, "updated_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_chat_sessions(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO chat_sessions
             (id, agent_id, title, archived, last_input_tokens, session_type,
              parent_session_id, source_bus_message_id, chain_depth,
              execution_state, finish_summary, terminal_error,
              created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            rusqlite::params![
                str_val(&r, "id"),
                str_val(&r, "agent_id"),
                str_val(&r, "title"),
                bool_val(&r, "archived"),
                r["last_input_tokens"].as_i64(),
                str_val(&r, "session_type"),
                opt_str(&r, "parent_session_id"),
                opt_str(&r, "source_bus_message_id"),
                int_val(&r, "chain_depth", 0),
                opt_str(&r, "execution_state"),
                opt_str(&r, "finish_summary"),
                opt_str(&r, "terminal_error"),
                str_val(&r, "created_at"),
                str_val(&r, "updated_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_chat_messages(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO chat_messages
             (id, session_id, role, content, token_count, is_compacted, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                str_val(&r, "id"),
                str_val(&r, "session_id"),
                str_val(&r, "role"),
                str_val(&r, "content"),
                r["token_count"].as_i64(),
                bool_val(&r, "is_compacted"),
                str_val(&r, "created_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_chat_compaction_summaries(
    conn: &rusqlite::Connection,
    rows: Vec<Value>,
) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO chat_compaction_summaries
             (id, session_id, summary_message_id, compacted_message_ids,
              original_token_count, summary_token_count, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                str_val(&r, "id"),
                str_val(&r, "session_id"),
                str_val(&r, "summary_message_id"),
                json_str(&r, "compacted_message_ids", "[]"),
                int_val(&r, "original_token_count", 0),
                int_val(&r, "summary_token_count", 0),
                str_val(&r, "created_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_bus_messages(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO bus_messages
             (id, from_agent_id, from_run_id, from_session_id,
              to_agent_id, to_run_id, to_session_id,
              kind, event_type, payload, status, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            rusqlite::params![
                str_val(&r, "id"),
                str_val(&r, "from_agent_id"),
                opt_str(&r, "from_run_id"),
                opt_str(&r, "from_session_id"),
                str_val(&r, "to_agent_id"),
                opt_str(&r, "to_run_id"),
                opt_str(&r, "to_session_id"),
                str_val(&r, "kind"),
                opt_str(&r, "event_type"),
                json_str(&r, "payload", "{}"),
                str_val(&r, "status"),
                str_val(&r, "created_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_bus_subscriptions(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO bus_subscriptions
             (id, subscriber_agent_id, source_agent_id, event_type, task_id,
              payload_template, enabled, max_chain_depth, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params![
                str_val(&r, "id"),
                str_val(&r, "subscriber_agent_id"),
                str_val(&r, "source_agent_id"),
                str_val(&r, "event_type"),
                str_val(&r, "task_id"),
                str_val(&r, "payload_template"),
                bool_val(&r, "enabled"),
                int_val(&r, "max_chain_depth", 10),
                str_val(&r, "created_at"),
                str_val(&r, "updated_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_users(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO users (id, name, is_default, created_at)
             VALUES (?1,?2,?3,?4)",
            rusqlite::params![
                str_val(&r, "id"),
                str_val(&r, "name"),
                bool_val(&r, "is_default"),
                str_val(&r, "created_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_memory_extraction_log(
    conn: &rusqlite::Connection,
    rows: Vec<Value>,
) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO memory_extraction_log
             (id, session_id, agent_id, memories_extracted, status, created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            rusqlite::params![
                str_val(&r, "id"),
                opt_str(&r, "session_id"),
                opt_str(&r, "agent_id"),
                int_val(&r, "memories_extracted", 0),
                str_val(&r, "status"),
                str_val(&r, "created_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
