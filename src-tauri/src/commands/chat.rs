use serde::Serialize;
use tracing::{ debug, info, warn };
use ulid::Ulid;

use crate::auth::{ AuthMode, AuthState };
use crate::db::cloud::CloudClientState;
use crate::db::DbPool;
use crate::events::emitter::{ emit_agent_iteration, emit_agent_tool_result, emit_chat_context_update };
use crate::executor::agent_tools::ToolExecutionContext;
use crate::executor::compaction;
use crate::executor::context::{ self, ContextMode, ContextRequest };
use crate::executor::engine::{ AgentSemaphores, ExecutorTx, SessionExecutionRegistry };
use crate::executor::keychain;
use crate::executor::permissions::{ self, PermissionRegistry };
use crate::executor::llm_provider::{ self, ChatMessage, ContentBlock, LlmConfig };
use crate::executor::memory::MemoryClient;
use crate::executor::session_agent;
use crate::executor::workspace;
use crate::memory_service::MemoryServiceState;
use crate::models::chat::ChatSession;

const MAX_TOKENS_PER_CALL: u32 = 4096;

// ─── Session CRUD ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_chat_sessions(
  agent_id: String,
  include_archived: Option<bool>,
  session_types: Option<Vec<String>>,
  db: tauri::State<'_, DbPool>
) -> Result<Vec<ChatSession>, String> {
  let pool = db.0.clone();
  let show_archived = include_archived.unwrap_or(false);
  let session_types = session_types.unwrap_or_default();

  tokio::task
    ::spawn_blocking(move || {
      let conn = pool.get().map_err(|e| e.to_string())?;
      let mut sql = String::from(
        "SELECT cs.id, cs.agent_id, cs.title, cs.archived, cs.session_type, cs.parent_session_id, cs.source_bus_message_id,
                cs.chain_depth, cs.execution_state, cs.finish_summary, cs.terminal_error,
                bm.from_agent_id, a.name,
                src.id, src.title,
                cs.created_at, cs.updated_at, cs.project_id
             FROM chat_sessions cs
             LEFT JOIN bus_messages bm ON bm.id = cs.source_bus_message_id
             LEFT JOIN agents a ON a.id = bm.from_agent_id
             LEFT JOIN chat_sessions src ON src.id = bm.from_session_id
             WHERE cs.agent_id = ?1"
      );
      let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(agent_id)];
      if !show_archived {
        sql.push_str(" AND cs.archived = 0");
      }
      if !session_types.is_empty() {
        let start_idx = params.len() + 1;
        let placeholders = (0..session_types.len())
          .map(|i| format!("?{}", start_idx + i))
          .collect::<Vec<_>>()
          .join(", ");
        sql.push_str(&format!(" AND cs.session_type IN ({})", placeholders));
        for session_type in session_types {
          params.push(Box::new(session_type));
        }
      }
      sql.push_str(" ORDER BY cs.updated_at DESC");

      let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
      let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
      let sessions = stmt
        .query_map(params_refs.as_slice(), |row| {
          Ok(ChatSession {
            id: row.get(0)?,
            agent_id: row.get(1)?,
            title: row.get(2)?,
            archived: row.get::<_, bool>(3)?,
            session_type: row.get(4)?,
            parent_session_id: row.get(5)?,
            source_bus_message_id: row.get(6)?,
            chain_depth: row.get(7)?,
            execution_state: row.get(8)?,
            finish_summary: row.get(9)?,
            terminal_error: row.get(10)?,
            source_agent_id: row.get(11)?,
            source_agent_name: row.get(12)?,
            source_session_id: row.get(13)?,
            source_session_title: row.get(14)?,
            created_at: row.get(15)?,
            updated_at: row.get(16)?,
            project_id: row.get(17)?,
          })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

      Ok(sessions)
    }).await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn create_chat_session(
  agent_id: String,
  title: Option<String>,
  session_type: Option<String>,
  project_id: Option<String>,
  db: tauri::State<'_, DbPool>,
  cloud: tauri::State<'_, CloudClientState>,
) -> Result<ChatSession, String> {
  let cloud = cloud.inner().clone();
  let pool = db.0.clone();

  let session: ChatSession = tokio::task
    ::spawn_blocking(move || -> Result<ChatSession, String> {
      let conn = pool.get().map_err(|e| e.to_string())?;
      let id = Ulid::new().to_string();
      let now = chrono::Utc::now().to_rfc3339();
      let title = title.unwrap_or_else(|| "New Chat".to_string());
      let session_type = session_type.unwrap_or_else(|| "user_chat".to_string());

      conn
        .execute(
          "INSERT INTO chat_sessions (
             id, agent_id, title, archived, session_type, parent_session_id, source_bus_message_id,
             chain_depth, execution_state, finish_summary, terminal_error, project_id, created_at, updated_at
           ) VALUES (?1, ?2, ?3, 0, ?4, NULL, NULL, 0, NULL, NULL, NULL, ?5, ?6, ?6)",
          rusqlite::params![id, agent_id, title, session_type, project_id, now]
        )
        .map_err(|e| e.to_string())?;

      Ok(ChatSession {
        id,
        agent_id,
        title,
        archived: false,
        session_type,
        parent_session_id: None,
        source_bus_message_id: None,
        chain_depth: 0,
        execution_state: None,
        finish_summary: None,
        terminal_error: None,
        source_agent_id: None,
        source_agent_name: None,
        source_session_id: None,
        source_session_title: None,
        created_at: now.clone(),
        updated_at: now,
        project_id,
      })
    }).await
    .map_err(|e| e.to_string())??;

  if let Some(client) = cloud.get() {
    let s = session.clone();
    tokio::spawn(async move {
      if let Err(e) = client.upsert_chat_session(&s).await {
        tracing::warn!("cloud upsert chat_session: {}", e);
      }
    });
  }
  Ok(session)
}

#[tauri::command]
pub async fn rename_chat_session(
  session_id: String,
  title: String,
  db: tauri::State<'_, DbPool>,
  cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
  let cloud = cloud.inner().clone();
  let pool = db.0.clone();
  let sid = session_id.clone();
  let now = chrono::Utc::now().to_rfc3339();
  let now2 = now.clone();
  let title2 = title.clone();
  tokio::task
    ::spawn_blocking(move || -> Result<(), String> {
      let conn = pool.get().map_err(|e| e.to_string())?;
      conn
        .execute(
          "UPDATE chat_sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
          rusqlite::params![title2, now2, sid]
        )
        .map_err(|e| e.to_string())?;
      Ok(())
    }).await
    .map_err(|e| e.to_string())??;
  if let Some(client) = cloud.get() {
    let id = session_id.clone();
    tokio::spawn(async move {
      let _ = client.patch_by_id("chat_sessions", &id,
        serde_json::json!({"title": title, "updated_at": now})).await;
    });
  }
  Ok(())
}

#[tauri::command]
pub async fn archive_chat_session(
  session_id: String,
  db: tauri::State<'_, DbPool>,
  cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
  let cloud = cloud.inner().clone();
  let pool = db.0.clone();
  let sid = session_id.clone();
  tokio::task
    ::spawn_blocking(move || -> Result<(), String> {
      let conn = pool.get().map_err(|e| e.to_string())?;
      let active_execution: Option<String> = conn.query_row(
        "SELECT execution_state FROM chat_sessions WHERE id = ?1",
        rusqlite::params![sid],
        |row| row.get(0)
      ).ok();
      if matches!(active_execution.as_deref(), Some("queued") | Some("running")) {
        return Err("cannot archive an active agent session".to_string());
      }
      let now = chrono::Utc::now().to_rfc3339();
      conn
        .execute(
          "UPDATE chat_sessions SET archived = 1, updated_at = ?1 WHERE id = ?2",
          rusqlite::params![now, sid]
        )
        .map_err(|e| e.to_string())?;
      conn
        .execute(
          "UPDATE chat_sessions SET archived = 1, updated_at = ?1 WHERE parent_session_id = ?2",
          rusqlite::params![now, sid]
        )
        .map_err(|e| e.to_string())?;
      conn
        .execute(
          "UPDATE chat_sessions SET archived = 1, updated_at = ?1 \
           WHERE id IN (SELECT bm.to_session_id FROM bus_messages bm WHERE bm.from_session_id = ?2 AND bm.to_session_id IS NOT NULL)",
          rusqlite::params![now, sid]
        )
        .map_err(|e| e.to_string())?;
      Ok(())
    }).await
    .map_err(|e| e.to_string())??;
  if let Some(client) = cloud.get() {
    let id = session_id.clone();
    let now = chrono::Utc::now().to_rfc3339();
    tokio::spawn(async move {
      let _ = client.patch_by_id("chat_sessions", &id,
        serde_json::json!({"archived": true, "updated_at": now})).await;
    });
  }
  Ok(())
}

#[tauri::command]
pub async fn unarchive_chat_session(
  session_id: String,
  db: tauri::State<'_, DbPool>,
  cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
  let cloud = cloud.inner().clone();
  let pool = db.0.clone();
  let sid = session_id.clone();
  tokio::task
    ::spawn_blocking(move || -> Result<(), String> {
      let conn = pool.get().map_err(|e| e.to_string())?;
      let now = chrono::Utc::now().to_rfc3339();
      conn
        .execute(
          "UPDATE chat_sessions SET archived = 0, updated_at = ?1 WHERE id = ?2",
          rusqlite::params![now, sid]
        )
        .map_err(|e| e.to_string())?;
      conn
        .execute(
          "UPDATE chat_sessions SET archived = 0, updated_at = ?1 WHERE parent_session_id = ?2",
          rusqlite::params![now, sid]
        )
        .map_err(|e| e.to_string())?;
      conn
        .execute(
          "UPDATE chat_sessions SET archived = 0, updated_at = ?1 \
           WHERE id IN (SELECT bm.to_session_id FROM bus_messages bm WHERE bm.from_session_id = ?2 AND bm.to_session_id IS NOT NULL)",
          rusqlite::params![now, sid]
        )
        .map_err(|e| e.to_string())?;
      Ok(())
    }).await
    .map_err(|e| e.to_string())??;
  if let Some(client) = cloud.get() {
    let id = session_id.clone();
    let now = chrono::Utc::now().to_rfc3339();
    tokio::spawn(async move {
      let _ = client.patch_by_id("chat_sessions", &id,
        serde_json::json!({"archived": false, "updated_at": now})).await;
    });
  }
  Ok(())
}

#[tauri::command]
pub async fn delete_chat_session(
  session_id: String,
  db: tauri::State<'_, DbPool>,
  cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
  let cloud = cloud.inner().clone();
  let pool = db.0.clone();
  let sid = session_id.clone();
  tokio::task
    ::spawn_blocking(move || -> Result<(), String> {
      let conn = pool.get().map_err(|e| e.to_string())?;
      let active_execution: Option<String> = conn.query_row(
        "SELECT execution_state FROM chat_sessions WHERE id = ?1",
        rusqlite::params![sid],
        |row| row.get(0)
      ).ok();
      if matches!(active_execution.as_deref(), Some("queued") | Some("running")) {
        return Err("cannot delete an active agent session".to_string());
      }
      conn
        .execute("DELETE FROM chat_sessions WHERE id = ?1", rusqlite::params![sid])
        .map_err(|e| e.to_string())?;
      Ok(())
    }).await
    .map_err(|e| e.to_string())??;
  if let Some(client) = cloud.get() {
    let id = session_id.clone();
    tokio::spawn(async move {
      let _ = client.delete_by_id("chat_sessions", &id).await;
    });
  }
  Ok(())
}

// ─── Messages ───────────────────────────────────────────────────────────────

/// A chat message with compaction metadata for the UI.
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessageWithMeta {
  pub role: String,
  pub content: Vec<ContentBlock>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub created_at: Option<String>,
  #[serde(rename = "isCompacted")]
  pub is_compacted: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedChatMessages {
  pub messages: Vec<ChatMessageWithMeta>,
  pub total_count: i64,
  pub has_more: bool,
}

#[tauri::command]
pub async fn get_chat_messages(
  session_id: String,
  limit: Option<i64>,
  offset: Option<i64>,
  db: tauri::State<'_, DbPool>
) -> Result<PaginatedChatMessages, String> {
  let pool = db.0.clone();

  tokio::task
    ::spawn_blocking(move || {
      let conn = pool.get().map_err(|e| e.to_string())?;

      let total_count: i64 = conn
        .query_row(
          "SELECT COUNT(*) FROM chat_messages WHERE session_id = ?1",
          rusqlite::params![session_id],
          |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

      let limit_val = limit.unwrap_or(0);
      let offset_val = offset.unwrap_or(0);

      let messages: Vec<ChatMessageWithMeta> = if limit_val > 0 {
        let mut stmt = conn
          .prepare(
            "SELECT role, content, created_at, is_compacted FROM (
               SELECT role, content, created_at, is_compacted
               FROM chat_messages WHERE session_id = ?1
               ORDER BY created_at DESC
               LIMIT ?2 OFFSET ?3
             ) sub ORDER BY created_at ASC"
          )
          .map_err(|e| e.to_string())?;

        let rows: Vec<ChatMessageWithMeta> = stmt
          .query_map(rusqlite::params![session_id, limit_val, offset_val], |row| {
            let role: String = row.get(0)?;
            let content_json: String = row.get(1)?;
            let created_at: Option<String> = row.get(2)?;
            let is_compacted: bool = row.get(3)?;
            Ok((role, content_json, created_at, is_compacted))
          })
          .map_err(|e| e.to_string())?
          .filter_map(|r| r.ok())
          .map(|(role, content_json, created_at, is_compacted)| {
            let content: Vec<ContentBlock> = serde_json::from_str(&content_json).unwrap_or_default();
            ChatMessageWithMeta { role, content, created_at, is_compacted }
          })
          .collect();
        rows
      } else {
        let mut stmt = conn
          .prepare(
            "SELECT role, content, created_at, is_compacted FROM chat_messages
                   WHERE session_id = ?1 ORDER BY created_at ASC"
          )
          .map_err(|e| e.to_string())?;

        let rows: Vec<ChatMessageWithMeta> = stmt
          .query_map(rusqlite::params![session_id], |row| {
            let role: String = row.get(0)?;
            let content_json: String = row.get(1)?;
            let created_at: Option<String> = row.get(2)?;
            let is_compacted: bool = row.get(3)?;
            Ok((role, content_json, created_at, is_compacted))
          })
          .map_err(|e| e.to_string())?
          .filter_map(|r| r.ok())
          .map(|(role, content_json, created_at, is_compacted)| {
            let content: Vec<ContentBlock> = serde_json::from_str(&content_json).unwrap_or_default();
            ChatMessageWithMeta { role, content, created_at, is_compacted }
          })
          .collect();
        rows
      };

      let has_more = if limit_val > 0 {
        (offset_val + limit_val) < total_count
      } else {
        false
      };

      Ok(PaginatedChatMessages { messages, total_count, has_more })
    }).await
    .map_err(|e| e.to_string())?
}

// ─── Send message (streaming) ───────────────────────────────────────────────

#[tauri::command]
pub async fn send_chat_message(
  session_id: String,
  content: String, // JSON-serialized Vec<ContentBlock>
  app: tauri::AppHandle,
  db: tauri::State<'_, DbPool>,
  executor_tx: tauri::State<'_, ExecutorTx>,
  agent_semaphores: tauri::State<'_, AgentSemaphores>,
  session_registry: tauri::State<'_, SessionExecutionRegistry>,
  permission_registry: tauri::State<'_, PermissionRegistry>,
  memory_state: tauri::State<'_, Option<MemoryServiceState>>,
  auth: tauri::State<'_, AuthState>,
  cloud: tauri::State<'_, CloudClientState>,
) -> Result<String, String> {
  let pool = db.0.clone();
  let stream_id = format!("chat:{}", session_id);
  let stream_id_ret = stream_id.clone();

  // Parse user content blocks
  let user_content: Vec<ContentBlock> = serde_json
    ::from_str(&content)
    .map_err(|e| format!("invalid content: {}", e))?;

  // Grab the cloud client before the blocking task so we can sync the user message afterwards
  let cloud_client = cloud.get();

  // Load session + history in blocking task
  let (agent_id, history, _session_title, chain_depth, user_msg_id, user_msg_now, user_msg_content_json) = {
    let pool = pool.clone();
    let sid = session_id.clone();
    let uc = user_content.clone();

    tokio::task
      ::spawn_blocking(
        move || -> Result<(String, Vec<ChatMessage>, String, i64, String, String, String), String> {
          let conn = pool.get().map_err(|e| e.to_string())?;

          // Get session
          let (agent_id, title, chain_depth): (String, String, i64) = conn
            .query_row(
              "SELECT agent_id, title, chain_depth FROM chat_sessions WHERE id = ?1",
              rusqlite::params![sid],
              |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            )
            .map_err(|e| format!("session not found: {}", e))?;

          // Load existing messages (exclude compacted ones — only active context goes to LLM)
          let mut stmt = conn
            .prepare(
              "SELECT role, content FROM chat_messages
                     WHERE session_id = ?1 AND is_compacted = 0 ORDER BY created_at ASC"
            )
            .map_err(|e| e.to_string())?;

          let mut messages: Vec<ChatMessage> = stmt
            .query_map(rusqlite::params![sid], |row| {
              let role: String = row.get(0)?;
              let content_json: String = row.get(1)?;
              Ok((role, content_json))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .map(|(role, content_json)| {
              let content: Vec<ContentBlock> = serde_json
                ::from_str(&content_json)
                .unwrap_or_default();
              ChatMessage { role, content, created_at: None }
            })
            .collect();

          // Save user message to DB
          let msg_id = Ulid::new().to_string();
          let now = chrono::Utc::now().to_rfc3339();
          let content_json = serde_json::to_string(&uc).map_err(|e| e.to_string())?;

          conn
            .execute(
              "INSERT INTO chat_messages (id, session_id, role, content, created_at)
                 VALUES (?1, ?2, 'user', ?3, ?4)",
              rusqlite::params![msg_id, sid, content_json, now]
            )
            .map_err(|e| e.to_string())?;

          // Update session timestamp
          conn
            .execute(
              "UPDATE chat_sessions SET updated_at = ?1 WHERE id = ?2",
              rusqlite::params![now, sid]
            )
            .map_err(|e| e.to_string())?;

          // Auto-title: if still "New Chat", use first text content
          if title == "New Chat" {
            let first_text = uc.iter().find_map(|b| {
              if let ContentBlock::Text { text } = b {
                Some(text.chars().take(60).collect::<String>())
              } else {
                None
              }
            });
            if let Some(t) = first_text {
              let _ = conn.execute(
                "UPDATE chat_sessions SET title = ?1 WHERE id = ?2",
                rusqlite::params![t, sid]
              );
            }
          }

          // Append user message to history
          messages.push(ChatMessage {
            role: "user".to_string(),
            content: uc,
            created_at: None,
          });

          Ok((agent_id, messages, title, chain_depth, msg_id, now, content_json))
        }
      ).await
      .map_err(|e| e.to_string())??
  };

  // Sync the initial user message to Supabase (was missing — only SQLite was written above)
  if let Some(client) = cloud_client.clone() {
    let sid_cloud = session_id.clone();
    let msg_id_cloud = user_msg_id.clone();
    let now_cloud = user_msg_now.clone();
    let content_json_cloud = user_msg_content_json.clone();
    tokio::spawn(async move {
      if let Err(e) = client
        .upsert_chat_message(&msg_id_cloud, &sid_cloud, "user", &content_json_cloud, &now_cloud)
        .await
      {
        warn!("cloud upsert initial user message: {}", e);
      }
    });
  }

  // Resolve memory user_id from auth state
  let memory_user_id = match auth.get().await {
    AuthMode::Cloud(session) => session.user_id,
    _ => "default_user".to_string(),
  };

  // Spawn the LLM call on a background task so the command returns immediately
  let db_bg = DbPool(pool.clone());
  let sid_bg = session_id.clone();
  let etx = executor_tx.0.clone();
  let semaphores = agent_semaphores.inner().clone();
  let registry = session_registry.inner().clone();
  let perm_registry = permission_registry.inner().clone();
  let mem_client = memory_state.as_ref().map(|s| s.client.clone());

  tauri::async_runtime::spawn(async move {
    if let Err(e) = do_llm_chat(
      &agent_id,
      history,
      &stream_id,
      &app,
      &db_bg,
      &sid_bg,
      &etx,
      chain_depth,
      semaphores,
      registry,
      perm_registry,
      mem_client.as_ref(),
      &memory_user_id,
      cloud_client.clone(),
    ).await {
      warn!("Chat LLM error: {}", e);
      // Emit finished with error info
      emit_agent_iteration(&app, &stream_id, 1, "finished", None, 0);
    }
  });

  Ok(stream_id_ret)
}

const MAX_CHAT_TOOL_ITERATIONS: u32 = 10;

/// Save a chat message to the DB.
async fn save_chat_message(
  pool: &r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
  session_id: &str,
  role: &str,
  content: &[ContentBlock],
  cloud: Option<std::sync::Arc<crate::db::cloud::SupabaseClient>>,
) -> Result<(), String> {
  let pool = pool.clone();
  let sid = session_id.to_string();
  let role = role.to_string();
  let content_json = serde_json::to_string(content).map_err(|e| e.to_string())?;

  let content_json_clone = content_json.clone();
  let sid_clone = sid.clone();
  let role_clone = role.clone();

  let (msg_id, now) = tokio::task::spawn_blocking(move || -> Result<(String, String), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let msg_id = Ulid::new().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute(
      "INSERT INTO chat_messages (id, session_id, role, content, created_at)
       VALUES (?1, ?2, ?3, ?4, ?5)",
      rusqlite::params![msg_id, sid, role, content_json, now],
    ).map_err(|e| e.to_string())?;

    conn.execute(
      "UPDATE chat_sessions SET updated_at = ?1 WHERE id = ?2",
      rusqlite::params![now, sid],
    ).map_err(|e| e.to_string())?;

    Ok((msg_id, now))
  }).await.map_err(|e| e.to_string())??;

  if let Some(client) = cloud {
    tokio::spawn(async move {
      if let Err(e) = client.upsert_chat_message(&msg_id, &sid_clone, &role_clone, &content_json_clone, &now).await {
        warn!("cloud upsert chat_message: {}", e);
      }
    });
  }

  Ok(())
}

/// Perform the actual LLM streaming call with tool execution support.
async fn do_llm_chat(
  agent_id: &str,
  messages: Vec<ChatMessage>,
  stream_id: &str,
  app: &tauri::AppHandle,
  db: &DbPool,
  session_id: &str,
  executor_tx: &tokio::sync::mpsc::UnboundedSender<crate::executor::engine::RunRequest>,
  chain_depth: i64,
  agent_semaphores: AgentSemaphores,
  session_registry: SessionExecutionRegistry,
  permission_registry: PermissionRegistry,
  memory_client: Option<&MemoryClient>,
  memory_user_id: &str,
  cloud_client: Option<std::sync::Arc<crate::db::cloud::SupabaseClient>>,
) -> Result<(), String> {
  // Load agent config
  let ws_config = workspace::load_agent_config(agent_id).unwrap_or_default();

  let provider_name = &ws_config.provider;
  let api_key = keychain
    ::retrieve_api_key(provider_name)
    .map_err(|_| format!("No API key for provider '{}'", provider_name))?;

  let provider = llm_provider::create_provider(provider_name, api_key)?;

  // Build context via pipeline (messages already loaded, pass them to avoid re-query)
  let pipeline = context::default_pipeline(memory_client.cloned());
  let ctx_request = ContextRequest {
    agent_id: agent_id.to_string(),
    mode: ContextMode::Chat,
    session_id: Some(session_id.to_string()),
    goal: None,
    ws_config: ws_config.clone(),
    existing_messages: Some(messages),
    is_sub_agent: false,
    chain_depth: 0,
    user_id: memory_user_id.to_string(),
  };
  let snapshot = pipeline.build(&ctx_request, db).await?;
  let mut messages = snapshot.messages;
  let tools = snapshot.tools;

  let context_window = snapshot.token_budget.context_window;

  let config = LlmConfig {
    model: ws_config.model.clone(),
    max_tokens: MAX_TOKENS_PER_CALL,
    temperature: Some(ws_config.temperature),
    system_prompt: snapshot.system_prompt,
  };

  let tool_ctx = ToolExecutionContext::new_with_bus(
    agent_id,
    stream_id,
    Some(session_id),
    chain_depth,
    db.clone(),
    executor_tx.clone(),
    app.clone(),
    agent_semaphores,
    session_registry,
  ).with_permission_registry(permission_registry.clone())
   .with_memory_client(memory_client.cloned())
   .with_memory_user_id(memory_user_id.to_string())
   .with_cloud_client(cloud_client.clone());
  let pool = db.0.clone();

  let mut cumulative_input_tokens: u32 = 0;
  let mut cumulative_output_tokens: u32 = 0;
  let mut iteration: u32 = 0;

  loop {
    iteration += 1;

    if iteration > MAX_CHAT_TOOL_ITERATIONS {
      info!(session_id = session_id, "Chat tool iteration limit reached");
      break;
    }

    debug!(
      session_id = session_id,
      message_count = messages.len(),
      iteration = iteration,
      "Chat LLM call (iteration {})",
      iteration,
    );

    emit_agent_iteration(app, stream_id, iteration, "llm_call", None,
      cumulative_input_tokens + cumulative_output_tokens);

    let response = provider
      .chat_streaming(&config, &messages, &tools, app, stream_id, iteration)
      .await?;

    cumulative_input_tokens += response.usage.input_tokens;
    cumulative_output_tokens += response.usage.output_tokens;

    // Save assistant response to DB
    save_chat_message(&pool, session_id, "assistant", &response.content, cloud_client.clone()).await?;

    match response.stop_reason {
      llm_provider::StopReason::EndTurn | llm_provider::StopReason::MaxTokens => {
        // Done — no tool calls, just a normal response
        messages.push(ChatMessage {
          role: "assistant".to_string(),
          content: response.content,
          created_at: None,
        });
        break;
      }

      llm_provider::StopReason::ToolUse => {
        // Add assistant message with tool_use blocks to conversation
        messages.push(ChatMessage {
          role: "assistant".to_string(),
          content: response.content.clone(),
          created_at: None,
        });

        // Execute each tool and collect results
        let mut tool_results: Vec<ContentBlock> = Vec::new();

        for block in &response.content {
          if let ContentBlock::ToolUse { id, name, input } = block {
            emit_agent_iteration(
              app, stream_id, iteration, "tool_exec",
              Some(name),
              cumulative_input_tokens + cumulative_output_tokens,
            );

            match permissions::execute_tool_with_permissions(&tool_ctx, name, input, app, stream_id, &permission_registry).await {
              Ok((result, _is_finish)) => {
                // Wrap tool output in data tags to signal untrusted content
                let wrapped = format!(
                  "<tool_result name=\"{}\" data_source=\"untrusted\">{}</tool_result>",
                  name, result
                );
                tool_results.push(ContentBlock::ToolResult {
                  tool_use_id: id.clone(),
                  content: wrapped,
                  is_error: false,
                });
                emit_agent_tool_result(app, stream_id, iteration, id, &result, false);
              }
              Err(err) => {
                let err_content = format!("Error: {}", err);
                tool_results.push(ContentBlock::ToolResult {
                  tool_use_id: id.clone(),
                  content: err_content.clone(),
                  is_error: true,
                });
                emit_agent_tool_result(app, stream_id, iteration, id, &err_content, true);
              }
            }
          }
        }

        // Save tool results to DB and add to conversation
        save_chat_message(&pool, session_id, "user", &tool_results, cloud_client.clone()).await?;

        messages.push(ChatMessage {
          role: "user".to_string(),
          content: tool_results,
          created_at: None,
        });

        // Loop back to call LLM again with tool results
      }
    }
  }

  let total_tokens = cumulative_input_tokens + cumulative_output_tokens;
  emit_agent_iteration(app, stream_id, iteration, "finished", None, total_tokens);

  // Emit context window usage update
  emit_chat_context_update(app, session_id, cumulative_input_tokens, cumulative_output_tokens, context_window);

  // Update last_input_tokens on session
  {
    let pool = pool.clone();
    let sid = session_id.to_string();
    let input_tokens = cumulative_input_tokens;
    let _ = tokio::task::spawn_blocking(move || {
      if let Ok(conn) = pool.get() {
        let now = chrono::Utc::now().to_rfc3339();
        let _ = conn.execute(
          "UPDATE chat_sessions SET last_input_tokens = ?1, updated_at = ?2 WHERE id = ?3",
          rusqlite::params![input_tokens, now, sid],
        );
      }
    }).await;
  }

  info!(session_id = session_id, "Chat complete ({} tokens, {} iterations)", total_tokens, iteration);

  // Check if compaction is needed
  let threshold = compaction::effective_threshold(&ws_config);
  if compaction::should_compact(cumulative_input_tokens, context_window, threshold) {
    info!(
      session_id = session_id,
      "Context usage {:.1}% exceeds threshold {:.0}%, triggering compaction",
      ((cumulative_input_tokens as f64) / (context_window as f64)) * 100.0,
      threshold * 100.0
    );

    let agent_id = agent_id.to_string();
    let session_id = session_id.to_string();
    let ws_config = ws_config.clone();
    let app = app.clone();
    let db = DbPool(db.0.clone());

    let compact_api_key = keychain
      ::retrieve_api_key(provider_name)
      .map_err(|_| format!("No API key for provider '{}'", provider_name))?;
    let compact_provider = llm_provider::create_provider(provider_name, compact_api_key)?;

    let compaction_user_id = memory_user_id.to_string();
    tauri::async_runtime::spawn(async move {
      match compaction::perform_compaction(
        &agent_id, &session_id, compact_provider.as_ref(),
        &ws_config, &app, &db, None, &compaction_user_id,
      ).await {
        Ok(()) => info!(session_id = %session_id, "Background compaction completed"),
        Err(e) => warn!(session_id = %session_id, "Background compaction failed: {}", e),
      }
    });
  }

  Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionExecutionStatus {
  pub session_id: String,
  pub execution_state: Option<String>,
  pub finish_summary: Option<String>,
  pub terminal_error: Option<String>,
}

#[tauri::command]
pub async fn get_session_execution(
  session_id: String,
  db: tauri::State<'_, DbPool>
) -> Result<SessionExecutionStatus, String> {
  let pool = db.0.clone();
  tokio::task::spawn_blocking(move || {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let sid = session_id.clone();
    conn.query_row(
      "SELECT execution_state, finish_summary, terminal_error FROM chat_sessions WHERE id = ?1",
      rusqlite::params![session_id],
      |row| {
        Ok(SessionExecutionStatus {
          session_id: sid.clone(),
          execution_state: row.get(0)?,
          finish_summary: row.get(1)?,
          terminal_error: row.get(2)?,
        })
      },
    ).map_err(|e| e.to_string())
  }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn cancel_agent_session(
  session_id: String,
  db: tauri::State<'_, DbPool>,
  session_registry: tauri::State<'_, SessionExecutionRegistry>,
) -> Result<(), String> {
  let pool = db.0.clone();
  let sid = session_id.clone();
  let session_type: String = tokio::task::spawn_blocking(move || {
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.query_row(
      "SELECT session_type FROM chat_sessions WHERE id = ?1",
      rusqlite::params![sid],
      |row| row.get(0),
    ).map_err(|e| e.to_string())
  }).await.map_err(|e| e.to_string())??;

  if !matches!(session_type.as_str(), "bus_message" | "sub_agent" | "pulse") {
    return Err("only bus_message, sub_agent, and pulse sessions can be cancelled".to_string());
  }

  session_registry.cancel(&session_id).await;
  let db_pool = DbPool(db.0.clone());
  session_agent::update_session_execution_state(
    &db_pool,
    &session_id,
    "cancelled",
    None,
    Some("Cancelled".to_string()),
  ).await
}

// ─── Context Usage Query ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
  pub input_tokens: u32,
  pub context_window_size: u32,
  pub usage_percent: f64,
}

#[tauri::command]
pub async fn get_context_usage(
  session_id: String,
  db: tauri::State<'_, DbPool>
) -> Result<ContextUsage, String> {
  let pool = db.0.clone();

  let (last_input_tokens, agent_id) = tokio::task
    ::spawn_blocking({
      let pool = pool.clone();
      let sid = session_id.clone();
      move || -> Result<(Option<u32>, String), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let row: (Option<u32>, String) = conn
          .query_row(
            "SELECT last_input_tokens, agent_id FROM chat_sessions WHERE id = ?1",
            rusqlite::params![sid],
            |row| Ok((row.get(0)?, row.get(1)?))
          )
          .map_err(|e| format!("session not found: {}", e))?;
        Ok(row)
      }
    }).await
    .map_err(|e| e.to_string())??;

  let ws_config = workspace::load_agent_config(&agent_id).unwrap_or_default();
  let context_window = compaction::effective_context_window(&ws_config);
  let input_tokens = last_input_tokens.unwrap_or(0);

  let usage_percent = if context_window > 0 {
    ((input_tokens as f64) / (context_window as f64)) * 100.0
  } else {
    0.0
  };

  Ok(ContextUsage {
    input_tokens,
    context_window_size: context_window,
    usage_percent,
  })
}

// ─── Manual Compaction ──────────────────────────────────────────────────────

#[tauri::command]
pub async fn compact_chat_session(
  session_id: String,
  app: tauri::AppHandle,
  db: tauri::State<'_, DbPool>,
  auth: tauri::State<'_, AuthState>,
) -> Result<(), String> {
  let pool = db.0.clone();

  // Look up agent_id for this session
  let agent_id: String = tokio::task
    ::spawn_blocking({
      let pool = pool.clone();
      let sid = session_id.clone();
      move || -> Result<String, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn
          .query_row(
            "SELECT agent_id FROM chat_sessions WHERE id = ?1",
            rusqlite::params![sid],
            |row| row.get(0)
          )
          .map_err(|e| format!("session not found: {}", e))
      }
    }).await
    .map_err(|e| e.to_string())??;

  let ws_config = workspace::load_agent_config(&agent_id).unwrap_or_default();
  let provider_name = &ws_config.provider;
  let api_key = keychain
    ::retrieve_api_key(provider_name)
    .map_err(|_| format!("No API key for provider '{}'", provider_name))?;
  let provider = llm_provider::create_provider(provider_name, api_key)?;

  let memory_user_id = match auth.get().await {
    AuthMode::Cloud(session) => session.user_id,
    _ => "default_user".to_string(),
  };

  let db_pool = DbPool(pool);
  compaction::perform_compaction(
    &agent_id,
    &session_id,
    provider.as_ref(),
    &ws_config,
    &app,
    &db_pool,
    None,
    &memory_user_id,
  ).await?;

  // Refetch and emit updated context usage
  let context_window = compaction::effective_context_window(&ws_config);
  emit_chat_context_update(&app, &session_id, 0, 0, context_window);

  info!(session_id = %session_id, "Manual compaction completed");
  Ok(())
}
