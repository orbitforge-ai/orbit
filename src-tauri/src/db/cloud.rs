//! Supabase cloud sync layer.
//!
//! Architecture: local SQLite is always the primary read/write store.
//! In cloud mode every mutating command also fires a background upsert to
//! Supabase (PostgREST REST API over reqwest).  On login the user's cloud
//! data is merged into local SQLite so a new device starts with their history.

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use serde_json::Value;
use std::sync::Arc;
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
    /// JWT access token — refreshed in place on 401.
    access_token: Arc<std::sync::RwLock<String>>,
    /// Supabase refresh token — used to obtain a new access token when expired.
    refresh_token: Arc<std::sync::RwLock<String>>,
    pub user_id: String,
    /// Stored so we can persist the refreshed session back to auth_state.json.
    email: String,
    data_dir: std::path::PathBuf,
    /// Prevents concurrent token refreshes (only one at a time).
    refresh_lock: Arc<tokio::sync::Mutex<()>>,
}

impl SupabaseClient {
    pub fn new(
        base_url: String,
        anon_key: String,
        access_token: String,
        refresh_token: String,
        user_id: String,
        email: String,
    ) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url,
            anon_key,
            access_token: Arc::new(std::sync::RwLock::new(access_token)),
            refresh_token: Arc::new(std::sync::RwLock::new(refresh_token)),
            user_id,
            email,
            data_dir: crate::data_dir(),
            refresh_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    fn token(&self) -> String {
        self.access_token.read().unwrap().clone()
    }

    /// Refresh the Supabase session using the stored refresh_token.
    /// Updates both tokens in-place and persists them to auth_state.json.
    async fn try_refresh(&self) -> Result<(), String> {
        let _guard = self.refresh_lock.lock().await;

        let refresh_token = self.refresh_token.read().unwrap().clone();
        if refresh_token.is_empty() {
            return Err("No refresh token available — please log in again".to_string());
        }

        let url = format!("{}/auth/v1/token?grant_type=refresh_token", self.base_url);
        let resp = self
            .http
            .post(&url)
            .header("apikey", &self.anon_key)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({ "refresh_token": refresh_token }))
            .send()
            .await
            .map_err(|e| format!("token refresh request: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("token refresh {status}: {body}"));
        }

        #[derive(serde::Deserialize)]
        struct RefreshResponse {
            access_token: String,
            refresh_token: String,
        }

        let data: RefreshResponse = resp
            .json()
            .await
            .map_err(|e| format!("parse refresh response: {e}"))?;

        *self.access_token.write().unwrap() = data.access_token.clone();
        *self.refresh_token.write().unwrap() = data.refresh_token.clone();

        let session = crate::auth::AuthSession {
            user_id: self.user_id.clone(),
            email: self.email.clone(),
            access_token: data.access_token,
            refresh_token: data.refresh_token,
        };
        crate::auth::persist_auth_state(&self.data_dir, &crate::auth::AuthMode::Cloud(session));

        tracing::info!("Supabase access token refreshed successfully");
        Ok(())
    }

    /// Send a request built by `build(token, anon_key)`.
    /// On 401, refreshes the session and retries once.
    async fn authed_send<F>(&self, build: F) -> Result<reqwest::Response, String>
    where
        F: Fn(&str, &str) -> reqwest::RequestBuilder,
    {
        let resp = build(&self.token(), &self.anon_key)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            tracing::debug!("Got 401 — attempting token refresh");
            self.try_refresh().await?;
            build(&self.token(), &self.anon_key)
                .send()
                .await
                .map_err(|e| e.to_string())
        } else {
            Ok(resp)
        }
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
        let http = self.http.clone();
        let resp = self
            .authed_send(move |token, ak| {
                http.get(&url)
                    .header("apikey", ak)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Accept", "application/json")
            })
            .await?;

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
        let http = self.http.clone();
        let resp = self
            .authed_send(move |token, ak| {
                http.post(&url)
                    .header("apikey", ak)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .header("Prefer", "resolution=merge-duplicates")
                    .json(&rows)
            })
            .await?;

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
        let http = self.http.clone();
        let resp = self
            .authed_send(move |token, ak| {
                http.patch(&url)
                    .header("apikey", ak)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .json(&updates)
            })
            .await?;

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
        let http = self.http.clone();
        let resp = self
            .authed_send(move |token, ak| {
                http.delete(&url)
                    .header("apikey", ak)
                    .header("Authorization", format!("Bearer {token}"))
            })
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("DELETE {table} {id} {status}: {text}"));
        }
        Ok(())
    }

    /// Push an API key to Supabase Vault via the `upsert_api_key` RPC.
    pub async fn upsert_api_key_in_vault(&self, provider: &str, key: &str) -> Result<(), String> {
        let url = format!("{}/rest/v1/rpc/upsert_api_key", self.base_url);
        let body = serde_json::json!({ "p_provider": provider, "p_key": key });
        let http = self.http.clone();
        let resp = self
            .authed_send(move |token, ak| {
                http.post(&url)
                    .header("apikey", ak)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .json(&body)
            })
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("upsert_api_key {provider} {status}: {text}"));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Entity-specific upsert helpers (build snake_case JSON bodies)
    // Note: all model structs use #[serde(rename_all = "camelCase")], so we
    // cannot use serde_json::to_value() directly — Supabase expects snake_case.
    // -----------------------------------------------------------------------

    pub async fn upsert_project(&self, p: &crate::models::project::Project) -> Result<(), String> {
        self.upsert_single(
            "projects",
            serde_json::json!({
                "user_id": self.user_id,
                "id": p.id,
                "name": p.name,
                "description": p.description,
                "created_at": p.created_at,
                "updated_at": p.updated_at,
            }),
        )
        .await
    }

    pub async fn upsert_agent(
        &self,
        a: &crate::models::agent::Agent,
        model_config: Option<&str>,
    ) -> Result<(), String> {
        let mut body = serde_json::json!({
            "user_id": self.user_id,
            "id": a.id,
            "name": a.name,
            "description": a.description,
            "state": a.state,
            "max_concurrent_runs": a.max_concurrent_runs,
            "heartbeat_at": a.heartbeat_at,
            "created_at": a.created_at,
            "updated_at": a.updated_at,
        });
        // Include model_config when provided (e.g. on create) to avoid a race
        // between the INSERT and a subsequent PATCH. On plain metadata updates
        // pass None so the stored config is never silently overwritten.
        if let Some(mc) = model_config {
            body["model_config"] = serde_json::Value::String(mc.to_string());
        }
        self.upsert_single("agents", body).await
    }

    /// PATCH only the model_config column for an agent (does not touch other fields).
    pub async fn patch_agent_model_config(
        &self,
        agent_id: &str,
        model_config_json: &str,
    ) -> Result<(), String> {
        self.patch_by_id(
            "agents",
            agent_id,
            serde_json::json!({ "model_config": model_config_json }),
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
                "project_id": t.project_id,
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
                "workflow_id": s.workflow_id,
                "target_kind": s.target_kind,
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
                "project_id": cs.project_id,
                "worktree_name": cs.worktree_name,
                "worktree_branch": cs.worktree_branch,
                "worktree_path": cs.worktree_path,
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

    pub async fn upsert_work_item(
        &self,
        w: &crate::models::work_item::WorkItem,
    ) -> Result<(), String> {
        let labels = serde_json::to_string(&w.labels).unwrap_or_else(|_| "[]".into());
        let metadata = serde_json::to_string(&w.metadata).unwrap_or_else(|_| "{}".into());
        self.upsert_single(
            "work_items",
            serde_json::json!({
                "user_id": self.user_id,
                "id": w.id,
                "project_id": w.project_id,
                "title": w.title,
                "description": w.description,
                "kind": w.kind,
                "column_id": w.column_id,
                "status": w.status,
                "priority": w.priority,
                "assignee_agent_id": w.assignee_agent_id,
                "created_by_agent_id": w.created_by_agent_id,
                "parent_work_item_id": w.parent_work_item_id,
                "position": w.position,
                "labels": labels,
                "metadata": metadata,
                "blocked_reason": w.blocked_reason,
                "started_at": w.started_at,
                "completed_at": w.completed_at,
                "created_at": w.created_at,
                "updated_at": w.updated_at,
            }),
        )
        .await
    }

    pub async fn upsert_project_board_column(
        &self,
        c: &crate::models::project_board_column::ProjectBoardColumn,
    ) -> Result<(), String> {
        self.upsert_single(
            "project_board_columns",
            serde_json::json!({
                "user_id": self.user_id,
                "id": c.id,
                "project_id": c.project_id,
                "name": c.name,
                "role": c.role,
                "is_default": c.is_default,
                "position": c.position,
                "created_at": c.created_at,
                "updated_at": c.updated_at,
            }),
        )
        .await
    }

    pub async fn upsert_work_item_comment(
        &self,
        c: &crate::models::work_item_comment::WorkItemComment,
    ) -> Result<(), String> {
        self.upsert_single(
            "work_item_comments",
            serde_json::json!({
                "user_id": self.user_id,
                "id": c.id,
                "work_item_id": c.work_item_id,
                "author_kind": c.author_kind,
                "author_agent_id": c.author_agent_id,
                "body": c.body,
                "created_at": c.created_at,
                "updated_at": c.updated_at,
            }),
        )
        .await
    }

    pub async fn upsert_project_workflow(
        &self,
        w: &crate::models::project_workflow::ProjectWorkflow,
    ) -> Result<(), String> {
        let graph = serde_json::to_string(&w.graph).unwrap_or_else(|_| "{}".into());
        let trigger_config =
            serde_json::to_string(&w.trigger_config).unwrap_or_else(|_| "{}".into());
        self.upsert_single(
            "project_workflows",
            serde_json::json!({
                "user_id": self.user_id,
                "id": w.id,
                "project_id": w.project_id,
                "name": w.name,
                "description": w.description,
                "enabled": w.enabled,
                "graph": graph,
                "trigger_kind": w.trigger_kind,
                "trigger_config": trigger_config,
                "version": w.version,
                "created_at": w.created_at,
                "updated_at": w.updated_at,
            }),
        )
        .await
    }

    pub async fn upsert_project_agent(
        &self,
        pa: &crate::models::project::ProjectAgent,
    ) -> Result<(), String> {
        self.upsert_single(
            "project_agents",
            serde_json::json!({
                "user_id": self.user_id,
                "project_id": pa.project_id,
                "agent_id": pa.agent_id,
                "is_default": pa.is_default,
                "added_at": pa.added_at,
            }),
        )
        .await
    }

    pub async fn delete_project_agent(
        &self,
        project_id: &str,
        agent_id: &str,
    ) -> Result<(), String> {
        let url = format!(
            "{}/rest/v1/project_agents?project_id=eq.{}&agent_id=eq.{}",
            self.base_url, project_id, agent_id
        );
        let http = self.http.clone();
        let resp = self
            .authed_send(move |token, ak| {
                http.delete(&url)
                    .header("apikey", ak)
                    .header("Authorization", format!("Bearer {token}"))
            })
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("DELETE project_agents {status}: {text}"));
        }
        Ok(())
    }

    pub async fn upsert_chat_message(
        &self,
        id: &str,
        session_id: &str,
        role: &str,
        content: &str,
        created_at: &str,
    ) -> Result<(), String> {
        self.upsert_single(
            "chat_messages",
            serde_json::json!({
                "user_id": self.user_id,
                "id": id,
                "session_id": session_id,
                "role": role,
                "content": content,
                "created_at": created_at,
            }),
        )
        .await
    }

    pub async fn upsert_message_reaction(
        &self,
        id: &str,
        message_id: &str,
        session_id: &str,
        emoji: &str,
        created_at: &str,
    ) -> Result<(), String> {
        self.upsert_single(
            "message_reactions",
            serde_json::json!({
                "user_id": self.user_id,
                "id": id,
                "message_id": message_id,
                "session_id": session_id,
                "emoji": emoji,
                "created_at": created_at,
            }),
        )
        .await
    }

    pub async fn upsert_chat_message_with_metadata(
        &self,
        id: &str,
        session_id: &str,
        role: &str,
        content: &str,
        token_count: Option<i64>,
        is_compacted: bool,
        created_at: &str,
    ) -> Result<(), String> {
        self.upsert_single(
            "chat_messages",
            serde_json::json!({
                "user_id": self.user_id,
                "id": id,
                "session_id": session_id,
                "role": role,
                "content": content,
                "token_count": token_count,
                "is_compacted": is_compacted,
                "created_at": created_at,
            }),
        )
        .await
    }

    pub async fn upsert_chat_compaction_summary(
        &self,
        id: &str,
        session_id: &str,
        summary_message_id: &str,
        compacted_message_ids: &str,
        original_token_count: Option<i64>,
        summary_token_count: i64,
        created_at: &str,
    ) -> Result<(), String> {
        let compacted_ids = serde_json::from_str::<Value>(compacted_message_ids)
            .unwrap_or_else(|_| serde_json::json!([]));
        self.upsert_single(
            "chat_compaction_summaries",
            serde_json::json!({
                "user_id": self.user_id,
                "id": id,
                "session_id": session_id,
                "summary_message_id": summary_message_id,
                "compacted_message_ids": compacted_ids,
                "original_token_count": original_token_count,
                "summary_token_count": summary_token_count,
                "created_at": created_at,
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

        let (
            agents,
            tasks,
            scheds,
            runs,
            convos,
            sessions,
            msgs,
            summaries,
            bus_msgs,
            bus_subs,
            users,
            mem_log,
        ) = rows;

        // Read project tables + reactions separately to keep tuple sizes manageable
        let user_id2 = self.user_id.clone();
        let p2 = pool.clone();
        let (
            projects,
            project_agents,
            project_board_columns,
            reactions,
            work_items,
            work_item_comments,
            project_workflows,
            workflow_runs,
        ) = tokio::task::spawn_blocking(move || {
            let conn = p2.get().map_err(|e| e.to_string())?;
            Ok::<_, String>((
                read_projects(&conn, &user_id2)?,
                read_project_agents(&conn, &user_id2)?,
                read_project_board_columns(&conn, &user_id2)?,
                read_message_reactions(&conn, &user_id2)?,
                read_work_items(&conn, &user_id2)?,
                read_work_item_comments(&conn, &user_id2)?,
                read_project_workflows(&conn, &user_id2)?,
                read_workflow_runs(&conn, &user_id2)?,
            ))
        })
        .await
        .map_err(|e| e.to_string())??;

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
        push!("projects", projects);
        push!("project_agents", project_agents);
        push!("project_board_columns", project_board_columns);
        push!("message_reactions", reactions);
        push!("work_items", work_items);
        push!("work_item_comments", work_item_comments);
        push!("project_workflows", project_workflows);
        push!("workflow_runs", workflow_runs);

        info!("Pushed local data to Supabase");
        Ok(())
    }

    /// Pull all cloud data into local SQLite (cloud wins on conflict for matching IDs).
    /// Called AFTER push so the device receives data from other devices.
    pub async fn pull_all_data(&self, pool: &Pool<SqliteConnectionManager>) -> Result<(), String> {
        self.pull_all_data_inner(pool).await.map(|_| ())
    }

    /// Like `pull_all_data` but returns row counts per table for diagnostics.
    pub async fn pull_all_data_with_counts(
        &self,
        pool: &Pool<SqliteConnectionManager>,
    ) -> Result<std::collections::HashMap<String, usize>, String> {
        self.pull_all_data_inner(pool).await
    }

    async fn pull_all_data_inner(
        &self,
        pool: &Pool<SqliteConnectionManager>,
    ) -> Result<std::collections::HashMap<String, usize>, String> {
        macro_rules! fetch {
            ($table:expr) => {{
                match self.get_table($table).await {
                    Ok(rows) => rows,
                    Err(e) => {
                        warn!("pull {} failed: {}", $table, e);
                        vec![]
                    }
                }
            }};
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
        let projects = fetch!("projects");
        let project_agents = fetch!("project_agents");
        let project_board_columns = fetch!("project_board_columns");
        let reactions = fetch!("message_reactions");
        let work_items = fetch!("work_items");
        let work_item_comments = fetch!("work_item_comments");
        let project_workflows = fetch!("project_workflows");
        let workflow_runs = fetch!("workflow_runs");

        let counts = std::collections::HashMap::from([
            ("agents".to_string(), agents.len()),
            ("tasks".to_string(), tasks.len()),
            ("schedules".to_string(), scheds.len()),
            ("runs".to_string(), runs.len()),
            ("agent_conversations".to_string(), convos.len()),
            ("chat_sessions".to_string(), sessions.len()),
            ("chat_messages".to_string(), msgs.len()),
            ("chat_compaction_summaries".to_string(), summaries.len()),
            ("bus_messages".to_string(), bus_msgs.len()),
            ("bus_subscriptions".to_string(), bus_subs.len()),
            ("users".to_string(), users.len()),
            ("memory_extraction_log".to_string(), mem_log.len()),
            ("projects".to_string(), projects.len()),
            ("project_agents".to_string(), project_agents.len()),
            ("project_board_columns".to_string(), project_board_columns.len()),
            ("message_reactions".to_string(), reactions.len()),
            ("work_items".to_string(), work_items.len()),
            ("work_item_comments".to_string(), work_item_comments.len()),
            ("project_workflows".to_string(), project_workflows.len()),
            ("workflow_runs".to_string(), workflow_runs.len()),
        ]);

        info!(
            "Cloud pull fetched: agents={} tasks={} sessions={} messages={} runs={} projects={}",
            agents.len(),
            tasks.len(),
            sessions.len(),
            msgs.len(),
            runs.len(),
            projects.len()
        );

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
            write_projects(&conn, projects)?;
            write_project_agents(&conn, project_agents)?;
            write_project_board_columns(&conn, project_board_columns)?;
            write_message_reactions(&conn, reactions)?;
            write_work_items(&conn, work_items)?;
            write_work_item_comments(&conn, work_item_comments)?;
            write_project_workflows(&conn, project_workflows)?;
            write_workflow_runs(&conn, workflow_runs)?;
            Ok::<(), String>(())
        })
        .await
        .map_err(|e| e.to_string())??;

        info!("Pulled cloud data into local SQLite");
        Ok(counts)
    }
}

// ---------------------------------------------------------------------------
// CloudClientState — Tauri managed state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct CloudClientState(pub Arc<std::sync::RwLock<Option<Arc<SupabaseClient>>>>);

/// Returns true when cloud sync is disabled via `DISABLE_CLOUD_SYNC=true|1`
/// in the .env file (read at compile time) or as a runtime env var.
pub fn cloud_sync_disabled() -> bool {
    const BUILD_FLAG: Option<&str> = option_env!("DISABLE_CLOUD_SYNC");
    let val = BUILD_FLAG
        .map(String::from)
        .or_else(|| std::env::var("DISABLE_CLOUD_SYNC").ok());
    matches!(val.as_deref(), Some("1") | Some("true") | Some("TRUE"))
}

impl CloudClientState {
    pub fn empty() -> Self {
        Self(Arc::new(std::sync::RwLock::new(None)))
    }

    /// Returns a clone of the current client (if any).
    /// Returns `None` when `DISABLE_CLOUD_SYNC=1` so all sync call-sites no-op.
    pub fn get(&self) -> Option<Arc<SupabaseClient>> {
        if cloud_sync_disabled() {
            return None;
        }
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

fn read_agents(conn: &rusqlite::Connection, user_id: &str) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, description, state, max_concurrent_runs, heartbeat_at,
                    model_config, created_at, updated_at FROM agents",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let agent_id: String = row.get(0)?;
            let mut model_config: String = row.get(6)?;
            // Migrate legacy empty model_config by reading from disk before pushing
            if model_config.is_empty() || model_config == "{}" {
                model_config = crate::executor::workspace::serialize_model_config(&agent_id)
                    .unwrap_or_else(|_| "{}".to_string());
            }
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": agent_id,
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
                    created_at, updated_at, project_id FROM tasks",
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
                "project_id": row.get::<_, Option<String>>(14)?,
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
            "SELECT id, task_id, workflow_id, target_kind, kind, config, enabled,
                    next_run_at, last_run_at, created_at, updated_at FROM schedules",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let config_str: String = row.get(5)?;
            let enabled: bool = row.get(6)?;
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "task_id": row.get::<_, Option<String>>(1)?,
                "workflow_id": row.get::<_, Option<String>>(2)?,
                "target_kind": row.get::<_, String>(3)?,
                "kind": row.get::<_, String>(4)?,
                "config": json_or_null(&config_str),
                "enabled": enabled,
                "next_run_at": row.get::<_, Option<String>>(7)?,
                "last_run_at": row.get::<_, Option<String>>(8)?,
                "created_at": row.get::<_, String>(9)?,
                "updated_at": row.get::<_, String>(10)?,
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
                    is_sub_agent, created_at, project_id FROM runs",
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
                "project_id": row.get::<_, Option<String>>(19)?,
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
                    created_at, updated_at, project_id, allow_sub_agents,
                    worktree_name, worktree_branch, worktree_path
             FROM chat_sessions",
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
                "project_id": row.get::<_, Option<String>>(14)?,
                "allow_sub_agents": row.get::<_, bool>(15)?,
                "worktree_name": row.get::<_, Option<String>>(16)?,
                "worktree_branch": row.get::<_, Option<String>>(17)?,
                "worktree_path": row.get::<_, Option<String>>(18)?,
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
                "original_token_count": row.get::<_, Option<i64>>(4)?,
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

fn read_projects(conn: &rusqlite::Connection, user_id: &str) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare("SELECT id, name, description, created_at, updated_at FROM projects")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "description": row.get::<_, Option<String>>(2)?,
                "created_at": row.get::<_, String>(3)?,
                "updated_at": row.get::<_, String>(4)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn read_project_agents(conn: &rusqlite::Connection, user_id: &str) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare("SELECT project_id, agent_id, is_default, added_at FROM project_agents")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let is_default: bool = row.get(2)?;
            Ok(serde_json::json!({
                "user_id": user_id,
                "project_id": row.get::<_, String>(0)?,
                "agent_id": row.get::<_, String>(1)?,
                "is_default": is_default,
                "added_at": row.get::<_, String>(3)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn read_project_board_columns(
    conn: &rusqlite::Connection,
    user_id: &str,
) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, project_id, name, role, is_default, position, created_at, updated_at
             FROM project_board_columns",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "project_id": row.get::<_, String>(1)?,
                "name": row.get::<_, String>(2)?,
                "role": row.get::<_, Option<String>>(3)?,
                "is_default": row.get::<_, bool>(4)?,
                "position": row.get::<_, f64>(5)?,
                "created_at": row.get::<_, String>(6)?,
                "updated_at": row.get::<_, String>(7)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn read_work_items(conn: &rusqlite::Connection, user_id: &str) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, project_id, title, description, kind, column_id, status, priority,
                    assignee_agent_id, created_by_agent_id, parent_work_item_id, position,
                    labels, metadata, blocked_reason, started_at, completed_at, created_at, updated_at
             FROM work_items",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "project_id": row.get::<_, String>(1)?,
                "title": row.get::<_, String>(2)?,
                "description": row.get::<_, Option<String>>(3)?,
                "kind": row.get::<_, String>(4)?,
                "column_id": row.get::<_, Option<String>>(5)?,
                "status": row.get::<_, String>(6)?,
                "priority": row.get::<_, i64>(7)?,
                "assignee_agent_id": row.get::<_, Option<String>>(8)?,
                "created_by_agent_id": row.get::<_, Option<String>>(9)?,
                "parent_work_item_id": row.get::<_, Option<String>>(10)?,
                "position": row.get::<_, f64>(11)?,
                "labels": row.get::<_, String>(12)?,
                "metadata": row.get::<_, String>(13)?,
                "blocked_reason": row.get::<_, Option<String>>(14)?,
                "started_at": row.get::<_, Option<String>>(15)?,
                "completed_at": row.get::<_, Option<String>>(16)?,
                "created_at": row.get::<_, String>(17)?,
                "updated_at": row.get::<_, String>(18)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn read_project_workflows(
    conn: &rusqlite::Connection,
    user_id: &str,
) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, project_id, name, description, enabled, graph,
                    trigger_kind, trigger_config, version, created_at, updated_at
             FROM project_workflows",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let enabled: bool = row.get(4)?;
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "project_id": row.get::<_, String>(1)?,
                "name": row.get::<_, String>(2)?,
                "description": row.get::<_, Option<String>>(3)?,
                "enabled": enabled,
                "graph": row.get::<_, String>(5)?,
                "trigger_kind": row.get::<_, String>(6)?,
                "trigger_config": row.get::<_, String>(7)?,
                "version": row.get::<_, i64>(8)?,
                "created_at": row.get::<_, String>(9)?,
                "updated_at": row.get::<_, String>(10)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn read_workflow_runs(conn: &rusqlite::Connection, user_id: &str) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, workflow_id, workflow_version, graph_snapshot, trigger_kind,
                    trigger_data, status, error, started_at, completed_at, created_at
             FROM workflow_runs",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let graph_str: String = row.get(3)?;
            let trigger_str: String = row.get(5)?;
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "workflow_id": row.get::<_, String>(1)?,
                "workflow_version": row.get::<_, i64>(2)?,
                "graph_snapshot": json_or_null(&graph_str),
                "trigger_kind": row.get::<_, String>(4)?,
                "trigger_data": json_or_null(&trigger_str),
                "status": row.get::<_, String>(6)?,
                "error": row.get::<_, Option<String>>(7)?,
                "started_at": row.get::<_, Option<String>>(8)?,
                "completed_at": row.get::<_, Option<String>>(9)?,
                "created_at": row.get::<_, String>(10)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn read_work_item_comments(
    conn: &rusqlite::Connection,
    user_id: &str,
) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, work_item_id, author_kind, author_agent_id, body, created_at, updated_at
             FROM work_item_comments",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "work_item_id": row.get::<_, String>(1)?,
                "author_kind": row.get::<_, String>(2)?,
                "author_agent_id": row.get::<_, Option<String>>(3)?,
                "body": row.get::<_, String>(4)?,
                "created_at": row.get::<_, String>(5)?,
                "updated_at": row.get::<_, String>(6)?,
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
    if row[key].as_bool().unwrap_or(false) {
        1
    } else {
        0
    }
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
        let agent_id = str_val(&r, "id");
        let model_config_str = json_str(&r, "model_config", "{}");
        conn.execute(
            "INSERT OR REPLACE INTO agents
             (id, name, description, state, max_concurrent_runs, heartbeat_at,
              model_config, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            rusqlite::params![
                agent_id,
                str_val(&r, "name"),
                opt_str(&r, "description"),
                str_val(&r, "state"),
                int_val(&r, "max_concurrent_runs", 5),
                opt_str(&r, "heartbeat_at"),
                model_config_str,
                str_val(&r, "created_at"),
                str_val(&r, "updated_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
        // Apply config.json + system_prompt.md to disk from the pulled model_config
        if let Err(e) =
            crate::executor::workspace::apply_model_config_to_disk(&agent_id, &model_config_str)
        {
            warn!("apply model_config to disk for agent {}: {}", agent_id, e);
        }
    }
    Ok(())
}

fn write_tasks(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO tasks
             (id, name, description, kind, config, max_duration_seconds, max_retries,
              retry_delay_seconds, concurrency_policy, tags, agent_id, enabled,
              created_at, updated_at, project_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
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
                opt_str(&r, "project_id"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_schedules(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
    for r in rows {
        let target_kind = r
            .get("target_kind")
            .and_then(|v| v.as_str())
            .unwrap_or("task")
            .to_string();
        conn.execute(
            "INSERT OR REPLACE INTO schedules
             (id, task_id, workflow_id, target_kind, kind, config, enabled,
              next_run_at, last_run_at, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            rusqlite::params![
                str_val(&r, "id"),
                opt_str(&r, "task_id"),
                opt_str(&r, "workflow_id"),
                target_kind,
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
              is_sub_agent, created_at, project_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
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
                opt_str(&r, "project_id"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_agent_conversations(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
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
              created_at, updated_at, project_id, allow_sub_agents,
              worktree_name, worktree_branch, worktree_path)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
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
                opt_str(&r, "project_id"),
                bool_val(&r, "allow_sub_agents"),
                opt_str(&r, "worktree_name"),
                opt_str(&r, "worktree_branch"),
                opt_str(&r, "worktree_path"),
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
        // original_token_count is nullable — preserve null instead of coercing to 0
        let original_token_count: Option<i64> = r["original_token_count"].as_i64();
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
                original_token_count,
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

fn write_projects(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO projects (id, name, description, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params![
                str_val(&r, "id"),
                str_val(&r, "name"),
                opt_str(&r, "description"),
                str_val(&r, "created_at"),
                str_val(&r, "updated_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_project_agents(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO project_agents (project_id, agent_id, is_default, added_at)
             VALUES (?1,?2,?3,?4)",
            rusqlite::params![
                str_val(&r, "project_id"),
                str_val(&r, "agent_id"),
                bool_val(&r, "is_default"),
                str_val(&r, "added_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_project_board_columns(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO project_board_columns (
                id, project_id, name, status, role, is_default, position, created_at, updated_at
             ) VALUES (?1,?2,?3,COALESCE(?4, 'backlog'),?4,?5,?6,?7,?8)",
            rusqlite::params![
                str_val(&r, "id"),
                str_val(&r, "project_id"),
                str_val(&r, "name"),
                opt_str(&r, "role"),
                bool_val(&r, "is_default"),
                r["position"].as_f64().unwrap_or(0.0),
                str_val(&r, "created_at"),
                str_val(&r, "updated_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_work_items(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO work_items (
                id, project_id, title, description, kind, column_id, status, priority,
                assignee_agent_id, created_by_agent_id, parent_work_item_id, position,
                labels, metadata, blocked_reason, started_at, completed_at, created_at, updated_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
            rusqlite::params![
                str_val(&r, "id"),
                str_val(&r, "project_id"),
                str_val(&r, "title"),
                opt_str(&r, "description"),
                str_val(&r, "kind"),
                opt_str(&r, "column_id"),
                str_val(&r, "status"),
                int_val(&r, "priority", 0),
                opt_str(&r, "assignee_agent_id"),
                opt_str(&r, "created_by_agent_id"),
                opt_str(&r, "parent_work_item_id"),
                r["position"].as_f64().unwrap_or(0.0),
                json_str(&r, "labels", "[]"),
                json_str(&r, "metadata", "{}"),
                opt_str(&r, "blocked_reason"),
                opt_str(&r, "started_at"),
                opt_str(&r, "completed_at"),
                str_val(&r, "created_at"),
                str_val(&r, "updated_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_project_workflows(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO project_workflows (
                id, project_id, name, description, enabled, graph,
                trigger_kind, trigger_config, version, created_at, updated_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            rusqlite::params![
                str_val(&r, "id"),
                str_val(&r, "project_id"),
                str_val(&r, "name"),
                opt_str(&r, "description"),
                bool_val(&r, "enabled"),
                json_str(
                    &r,
                    "graph",
                    "{\"nodes\":[],\"edges\":[],\"schemaVersion\":1}"
                ),
                str_val(&r, "trigger_kind"),
                json_str(&r, "trigger_config", "{}"),
                int_val(&r, "version", 1),
                str_val(&r, "created_at"),
                str_val(&r, "updated_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_workflow_runs(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO workflow_runs (
                id, workflow_id, workflow_version, graph_snapshot, trigger_kind,
                trigger_data, status, error, started_at, completed_at, created_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            rusqlite::params![
                str_val(&r, "id"),
                str_val(&r, "workflow_id"),
                int_val(&r, "workflow_version", 1),
                json_str(&r, "graph_snapshot", "{}"),
                str_val(&r, "trigger_kind"),
                json_str(&r, "trigger_data", "{}"),
                str_val(&r, "status"),
                opt_str(&r, "error"),
                opt_str(&r, "started_at"),
                opt_str(&r, "completed_at"),
                str_val(&r, "created_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_work_item_comments(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO work_item_comments (
                id, work_item_id, author_kind, author_agent_id, body, created_at, updated_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                str_val(&r, "id"),
                str_val(&r, "work_item_id"),
                str_val(&r, "author_kind"),
                opt_str(&r, "author_agent_id"),
                str_val(&r, "body"),
                str_val(&r, "created_at"),
                str_val(&r, "updated_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn read_message_reactions(
    conn: &rusqlite::Connection,
    user_id: &str,
) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare("SELECT id, message_id, session_id, emoji, created_at FROM message_reactions")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "user_id": user_id,
                "id": row.get::<_, String>(0)?,
                "message_id": row.get::<_, String>(1)?,
                "session_id": row.get::<_, String>(2)?,
                "emoji": row.get::<_, String>(3)?,
                "created_at": row.get::<_, String>(4)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn write_message_reactions(conn: &rusqlite::Connection, rows: Vec<Value>) -> Result<(), String> {
    for r in rows {
        conn.execute(
            "INSERT OR REPLACE INTO message_reactions
             (id, message_id, session_id, emoji, created_at)
             VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params![
                str_val(&r, "id"),
                str_val(&r, "message_id"),
                str_val(&r, "session_id"),
                str_val(&r, "emoji"),
                str_val(&r, "created_at"),
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
