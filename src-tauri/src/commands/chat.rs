use serde::Serialize;
use tracing::{ debug, info, warn };
use ulid::Ulid;

use crate::db::DbPool;
use crate::events::emitter::{ emit_agent_iteration, emit_agent_tool_result, emit_chat_context_update };
use crate::executor::agent_tools::{ self, ToolExecutionContext };
use crate::executor::compaction;
use crate::executor::context::{ self, ContextMode, ContextRequest };
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

#[tauri::command]
pub async fn get_chat_messages(
  session_id: String,
  db: tauri::State<'_, DbPool>
) -> Result<Vec<ChatMessageWithMeta>, String> {
  let pool = db.0.clone();

  tokio::task
    ::spawn_blocking(move || {
      let conn = pool.get().map_err(|e| e.to_string())?;
      let mut stmt = conn
        .prepare(
          "SELECT role, content, created_at, is_compacted FROM chat_messages
                 WHERE session_id = ?1 ORDER BY created_at ASC"
        )
        .map_err(|e| e.to_string())?;

      let messages = stmt
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

const MAX_CHAT_TOOL_ITERATIONS: u32 = 10;

/// Save a chat message to the DB.
async fn save_chat_message(
  pool: &r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
  session_id: &str,
  role: &str,
  content: &[ContentBlock],
) -> Result<(), String> {
  let pool = pool.clone();
  let sid = session_id.to_string();
  let role = role.to_string();
  let content_json = serde_json::to_string(content).map_err(|e| e.to_string())?;

  tokio::task::spawn_blocking(move || -> Result<(), String> {
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

    Ok(())
  }).await.map_err(|e| e.to_string())??;

  Ok(())
}

/// Perform the actual LLM streaming call with tool execution support.
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

  let provider_name = &ws_config.provider;
  let api_key = keychain
    ::retrieve_api_key(provider_name)
    .map_err(|_| format!("No API key for provider '{}'", provider_name))?;

  let provider = llm_provider::create_provider(provider_name, api_key)?;

  // Build context via pipeline (messages already loaded, pass them to avoid re-query)
  let pipeline = context::default_pipeline();
  let ctx_request = ContextRequest {
    agent_id: agent_id.to_string(),
    mode: ContextMode::Chat,
    session_id: Some(session_id.to_string()),
    run_id: stream_id.to_string(),
    goal: None,
    ws_config: ws_config.clone(),
    existing_messages: Some(messages),
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

  let tool_ctx = ToolExecutionContext::new(agent_id);
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
    save_chat_message(&pool, session_id, "assistant", &response.content).await?;

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

            match agent_tools::execute_tool(&tool_ctx, name, input, app, stream_id).await {
              Ok((result, _is_finish)) => {
                tool_results.push(ContentBlock::ToolResult {
                  tool_use_id: id.clone(),
                  content: result.clone(),
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
        save_chat_message(&pool, session_id, "user", &tool_results).await?;

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

    tauri::async_runtime::spawn(async move {
      match compaction::perform_compaction(
        &agent_id, &session_id, compact_provider.as_ref(),
        &ws_config, &app, &db,
      ).await {
        Ok(()) => info!(session_id = %session_id, "Background compaction completed"),
        Err(e) => warn!(session_id = %session_id, "Background compaction failed: {}", e),
      }
    });
  }

  Ok(())
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
  db: tauri::State<'_, DbPool>
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

  let db_pool = DbPool(pool);
  compaction::perform_compaction(
    &agent_id,
    &session_id,
    provider.as_ref(),
    &ws_config,
    &app,
    &db_pool
  ).await?;

  // Refetch and emit updated context usage
  let context_window = compaction::effective_context_window(&ws_config);
  emit_chat_context_update(&app, &session_id, 0, 0, context_window);

  info!(session_id = %session_id, "Manual compaction completed");
  Ok(())
}
