use tracing::{ info, warn };
use ulid::Ulid;

use crate::db::DbPool;
use crate::events::emitter::emit_agent_iteration;
use crate::executor::keychain;
use crate::executor::llm_provider::{ self, ChatMessage, ContentBlock, LlmConfig };
use crate::executor::workspace;
use crate::models::chat::ChatSession;

const MAX_TOKENS_PER_CALL: u32 = 4096;

// ─── Session CRUD ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_chat_sessions(
  agent_id: String,
  include_archived: Option<bool>,
  db: tauri::State<'_, DbPool>
) -> Result<Vec<ChatSession>, String> {
  let pool = db.0.clone();
  let show_archived = include_archived.unwrap_or(false);

  tokio::task
    ::spawn_blocking(move || {
      let conn = pool.get().map_err(|e| e.to_string())?;
      let mut sql = String::from(
        "SELECT id, agent_id, title, archived, created_at, updated_at
             FROM chat_sessions WHERE agent_id = ?1"
      );
      if !show_archived {
        sql.push_str(" AND archived = 0");
      }
      sql.push_str(" ORDER BY updated_at DESC");

      let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
      let sessions = stmt
        .query_map(rusqlite::params![agent_id], |row| {
          Ok(ChatSession {
            id: row.get(0)?,
            agent_id: row.get(1)?,
            title: row.get(2)?,
            archived: row.get::<_, bool>(3)?,
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
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
  db: tauri::State<'_, DbPool>
) -> Result<ChatSession, String> {
  let pool = db.0.clone();

  tokio::task
    ::spawn_blocking(move || {
      let conn = pool.get().map_err(|e| e.to_string())?;
      let id = Ulid::new().to_string();
      let now = chrono::Utc::now().to_rfc3339();
      let title = title.unwrap_or_else(|| "New Chat".to_string());

      conn
        .execute(
          "INSERT INTO chat_sessions (id, agent_id, title, archived, created_at, updated_at)
             VALUES (?1, ?2, ?3, 0, ?4, ?4)",
          rusqlite::params![id, agent_id, title, now]
        )
        .map_err(|e| e.to_string())?;

      Ok(ChatSession {
        id,
        agent_id,
        title,
        archived: false,
        created_at: now.clone(),
        updated_at: now,
      })
    }).await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn rename_chat_session(
  session_id: String,
  title: String,
  db: tauri::State<'_, DbPool>
) -> Result<(), String> {
  let pool = db.0.clone();
  tokio::task
    ::spawn_blocking(move || {
      let conn = pool.get().map_err(|e| e.to_string())?;
      let now = chrono::Utc::now().to_rfc3339();
      conn
        .execute(
          "UPDATE chat_sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
          rusqlite::params![title, now, session_id]
        )
        .map_err(|e| e.to_string())?;
      Ok(())
    }).await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn archive_chat_session(
  session_id: String,
  db: tauri::State<'_, DbPool>
) -> Result<(), String> {
  let pool = db.0.clone();
  tokio::task
    ::spawn_blocking(move || {
      let conn = pool.get().map_err(|e| e.to_string())?;
      let now = chrono::Utc::now().to_rfc3339();
      conn
        .execute(
          "UPDATE chat_sessions SET archived = 1, updated_at = ?1 WHERE id = ?2",
          rusqlite::params![now, session_id]
        )
        .map_err(|e| e.to_string())?;
      Ok(())
    }).await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unarchive_chat_session(
  session_id: String,
  db: tauri::State<'_, DbPool>
) -> Result<(), String> {
  let pool = db.0.clone();
  tokio::task
    ::spawn_blocking(move || {
      let conn = pool.get().map_err(|e| e.to_string())?;
      let now = chrono::Utc::now().to_rfc3339();
      conn
        .execute(
          "UPDATE chat_sessions SET archived = 0, updated_at = ?1 WHERE id = ?2",
          rusqlite::params![now, session_id]
        )
        .map_err(|e| e.to_string())?;
      Ok(())
    }).await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_chat_session(
  session_id: String,
  db: tauri::State<'_, DbPool>
) -> Result<(), String> {
  let pool = db.0.clone();
  tokio::task
    ::spawn_blocking(move || {
      let conn = pool.get().map_err(|e| e.to_string())?;
      conn
        .execute("DELETE FROM chat_sessions WHERE id = ?1", rusqlite::params![session_id])
        .map_err(|e| e.to_string())?;
      Ok(())
    }).await
    .map_err(|e| e.to_string())?
}

// ─── Messages ───────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_chat_messages(
  session_id: String,
  db: tauri::State<'_, DbPool>
) -> Result<Vec<ChatMessage>, String> {
  let pool = db.0.clone();

  tokio::task
    ::spawn_blocking(move || {
      let conn = pool.get().map_err(|e| e.to_string())?;
      let mut stmt = conn
        .prepare(
          "SELECT role, content FROM chat_messages
                 WHERE session_id = ?1 ORDER BY created_at ASC"
        )
        .map_err(|e| e.to_string())?;

      let messages = stmt
        .query_map(rusqlite::params![session_id], |row| {
          let role: String = row.get(0)?;
          let content_json: String = row.get(1)?;
          Ok((role, content_json))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .map(|(role, content_json)| {
          let content: Vec<ContentBlock> = serde_json::from_str(&content_json).unwrap_or_default();
          ChatMessage { role, content }
        })
        .collect();

      Ok(messages)
    }).await
    .map_err(|e| e.to_string())?
}

// ─── Send message (streaming) ───────────────────────────────────────────────

#[tauri::command]
pub async fn send_chat_message(
  session_id: String,
  content: String, // JSON-serialized Vec<ContentBlock>
  app: tauri::AppHandle,
  db: tauri::State<'_, DbPool>
) -> Result<String, String> {
  let pool = db.0.clone();
  let stream_id = format!("chat:{}", session_id);
  let stream_id_ret = stream_id.clone();

  // Parse user content blocks
  let user_content: Vec<ContentBlock> = serde_json
    ::from_str(&content)
    .map_err(|e| format!("invalid content: {}", e))?;

  // Load session + history in blocking task
  let (agent_id, history, _session_title) = {
    let pool = pool.clone();
    let sid = session_id.clone();
    let uc = user_content.clone();

    tokio::task
      ::spawn_blocking(
        move || -> Result<(String, Vec<ChatMessage>, String), String> {
          let conn = pool.get().map_err(|e| e.to_string())?;

          // Get session
          let (agent_id, title): (String, String) = conn
            .query_row(
              "SELECT agent_id, title FROM chat_sessions WHERE id = ?1",
              rusqlite::params![sid],
              |row| Ok((row.get(0)?, row.get(1)?))
            )
            .map_err(|e| format!("session not found: {}", e))?;

          // Load existing messages
          let mut stmt = conn
            .prepare(
              "SELECT role, content FROM chat_messages
                     WHERE session_id = ?1 ORDER BY created_at ASC"
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
              ChatMessage { role, content }
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
          });

          Ok((agent_id, messages, title))
        }
      ).await
      .map_err(|e| e.to_string())??
  };

  // Spawn the LLM call on a background task so the command returns immediately
  let db_bg = DbPool(pool.clone());
  let sid_bg = session_id.clone();

  tauri::async_runtime::spawn(async move {
    if let Err(e) = do_llm_chat(&agent_id, history, &stream_id, &app, &db_bg, &sid_bg).await {
      warn!("Chat LLM error: {}", e);
      // Emit finished with error info
      emit_agent_iteration(&app, &stream_id, 1, "finished", None, 0);
    }
  });

  Ok(stream_id_ret)
}

/// Perform the actual LLM streaming call and save the response.
async fn do_llm_chat(
  agent_id: &str,
  messages: Vec<ChatMessage>,
  stream_id: &str,
  app: &tauri::AppHandle,
  db: &DbPool,
  session_id: &str
) -> Result<(), String> {
  // Load agent config
  let ws_config = workspace::load_agent_config(agent_id).unwrap_or_default();
  let system_prompt = workspace
    ::read_workspace_file(agent_id, "system_prompt.md")
    .unwrap_or_else(|_| "You are a helpful assistant.".to_string());

  let provider_name = &ws_config.provider;
  let api_key = keychain
    ::retrieve_api_key(provider_name)
    .map_err(|_| format!("No API key for provider '{}'", provider_name))?;

  let provider = llm_provider::create_provider(provider_name, api_key)?;

  let config = LlmConfig {
    model: ws_config.model.clone(),
    max_tokens: MAX_TOKENS_PER_CALL,
    temperature: Some(ws_config.temperature),
    system_prompt,
  };

  emit_agent_iteration(app, stream_id, 1, "llm_call", None, 0);

  let response = provider.chat_streaming(&config, &messages, &[], app, stream_id, 1).await?;

  let total_tokens = response.usage.input_tokens + response.usage.output_tokens;
  emit_agent_iteration(app, stream_id, 1, "finished", None, total_tokens);

  // Save assistant response to DB
  let content_json = serde_json::to_string(&response.content).map_err(|e| e.to_string())?;

  let pool = db.0.clone();
  let sid = session_id.to_string();

  tokio::task
    ::spawn_blocking(
      move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let msg_id = Ulid::new().to_string();
        let now = chrono::Utc::now().to_rfc3339();

        conn
          .execute(
            "INSERT INTO chat_messages (id, session_id, role, content, created_at)
             VALUES (?1, ?2, 'assistant', ?3, ?4)",
            rusqlite::params![msg_id, sid, content_json, now]
          )
          .map_err(|e| e.to_string())?;

        conn
          .execute(
            "UPDATE chat_sessions SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now, sid]
          )
          .map_err(|e| e.to_string())?;

        Ok(())
      }
    ).await
    .map_err(|e| e.to_string())??;

  info!(session_id = session_id, "Chat response saved ({} tokens)", total_tokens);
  Ok(())
}
