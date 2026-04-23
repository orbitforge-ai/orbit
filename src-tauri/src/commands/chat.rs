use regex::Regex;
use serde::Deserialize;
use serde::Serialize;
use tauri::Manager;
use tracing::{debug, info, warn};
use ulid::Ulid;

use crate::auth::{AuthMode, AuthState};
use crate::db::cloud::CloudClientState;
use crate::db::DbPool;
use crate::events::emitter::{
    emit_agent_iteration, emit_agent_tool_result, emit_chat_context_update,
};
use crate::executor::agent_tools::ToolExecutionContext;
use crate::executor::compaction;
use crate::executor::context::{self, ContextMode, ContextRequest};
use crate::executor::engine::{
    AgentSemaphores, ExecutorTx, SessionExecutionRegistry, UserQuestionRegistry,
};
use crate::executor::keychain;
use crate::executor::llm_provider::{self, ChatMessage, ContentBlock, LlmConfig};
use crate::executor::memory::MemoryClient;
use crate::executor::permissions::{self, PermissionRegistry};
use crate::executor::session_agent;
use crate::executor::session_worktree;
use crate::executor::skills;
use crate::executor::workspace;
use crate::memory_service::MemoryServiceState;
use crate::models::chat::ChatSession;

const MAX_TOKENS_PER_CALL: u32 = 4096;
const CHAT_CANCEL_POLL_INTERVAL_MS: u64 = 100;
const SKILL_MENTION_PATTERN: &str = r#"[@#]\[[^\]]+\]\(mention:skill:(?P<skill_name>[^)]+)\)"#;

fn can_cancel_chat_session(session_type: &str) -> bool {
    matches!(
        session_type,
        "user_chat" | "bus_message" | "sub_agent" | "pulse"
    )
}

async fn is_chat_session_cancelled(
    session_id: &str,
    db: &DbPool,
    session_registry: &SessionExecutionRegistry,
) -> bool {
    if session_registry.is_cancelled(session_id).await {
        return true;
    }

    let pool = db.0.clone();
    let session_id = session_id.to_string();
    tokio::task::spawn_blocking(move || -> bool {
        let conn = match pool.get() {
            Ok(conn) => conn,
            Err(_) => return false,
        };
        let state: Option<String> = conn
            .query_row(
                "SELECT execution_state FROM chat_sessions WHERE id = ?1",
                rusqlite::params![session_id],
                |row| row.get(0),
            )
            .ok();
        matches!(state.as_deref(), Some("cancelled"))
    })
    .await
    .unwrap_or(false)
}

async fn chat_streaming_with_cancellation(
    provider: &dyn llm_provider::LlmProvider,
    config: &LlmConfig,
    messages: &[ChatMessage],
    tools: &[llm_provider::ToolDefinition],
    app: &tauri::AppHandle,
    stream_id: &str,
    iteration: u32,
    session_id: &str,
    db: &DbPool,
    session_registry: &SessionExecutionRegistry,
) -> Result<llm_provider::LlmResponse, String> {
    let stream_future = provider.chat_streaming(config, messages, tools, app, stream_id, iteration);
    tokio::pin!(stream_future);

    let mut cancellation_poll = tokio::time::interval(tokio::time::Duration::from_millis(
        CHAT_CANCEL_POLL_INTERVAL_MS,
    ));

    loop {
        tokio::select! {
            response = &mut stream_future => return response,
            _ = cancellation_poll.tick() => {
                if is_chat_session_cancelled(session_id, db, session_registry).await {
                    return Err("cancelled".to_string());
                }
            }
        }
    }
}

fn extract_skill_mentions(blocks: &[ContentBlock]) -> Vec<String> {
    let regex = match Regex::new(SKILL_MENTION_PATTERN) {
        Ok(regex) => regex,
        Err(err) => {
            warn!("failed to compile skill mention regex: {}", err);
            return Vec::new();
        }
    };

    let mut seen = std::collections::BTreeSet::new();
    for block in blocks {
        let ContentBlock::Text { text } = block else {
            continue;
        };
        for caps in regex.captures_iter(text) {
            let Some(skill_name) = caps.name("skill_name") else {
                continue;
            };
            seen.insert(skill_name.as_str().to_string());
        }
    }

    seen.into_iter().collect()
}

fn activate_skill_mentions_for_session(
    db: &DbPool,
    session_id: &str,
    agent_id: &str,
    disabled_skills: &[String],
    blocks: &[ContentBlock],
) -> Result<Vec<String>, String> {
    let mentioned_skill_names = extract_skill_mentions(blocks);
    if mentioned_skill_names.is_empty() {
        return Ok(Vec::new());
    }

    let mut activated = Vec::new();
    for skill_name in mentioned_skill_names {
        match skills::load_skill(agent_id, &skill_name, disabled_skills) {
            Ok(loaded_skill) => {
                skills::upsert_active_skill(
                    db,
                    session_id,
                    &skill_name,
                    &loaded_skill.instructions,
                    loaded_skill.metadata.source_path.as_deref(),
                )?;
                activated.push(skill_name);
            }
            Err(err) => {
                warn!(
                    session_id = session_id,
                    skill = skill_name,
                    error = %err,
                    "failed to activate mentioned skill"
                );
            }
        }
    }

    Ok(activated)
}

// ─── Session CRUD ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_chat_sessions(
    agent_id: String,
    include_archived: Option<bool>,
    session_types: Option<Vec<String>>,
    project_id: Option<String>,
    db: tauri::State<'_, DbPool>,
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
                cs.created_at, cs.updated_at, cs.project_id,
                cs.worktree_name, cs.worktree_branch, cs.worktree_path
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
      if let Some(pid) = project_id {
        let idx = params.len() + 1;
        sql.push_str(&format!(" AND cs.project_id = ?{}", idx));
        params.push(Box::new(pid));
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
            worktree_name: row.get(18)?,
            worktree_branch: row.get(19)?,
            worktree_path: row.get(20)?,
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
      if let Some(pid) = project_id.as_deref() {
        crate::commands::projects::assert_agent_in_project_sync(&conn, pid, &agent_id)?;
      }
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
        worktree_name: None,
        worktree_branch: None,
        worktree_path: None,
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
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE chat_sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![title2, now2, sid],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;
    if let Some(client) = cloud.get() {
        let id = session_id.clone();
        tokio::spawn(async move {
            let _ = client
                .patch_by_id(
                    "chat_sessions",
                    &id,
                    serde_json::json!({"title": title, "updated_at": now}),
                )
                .await;
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
            let _ = client
                .patch_by_id(
                    "chat_sessions",
                    &id,
                    serde_json::json!({"archived": true, "updated_at": now}),
                )
                .await;
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
            let _ = client
                .patch_by_id(
                    "chat_sessions",
                    &id,
                    serde_json::json!({"archived": false, "updated_at": now}),
                )
                .await;
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
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let active_execution: Option<String> = conn
            .query_row(
                "SELECT execution_state FROM chat_sessions WHERE id = ?1",
                rusqlite::params![sid],
                |row| row.get(0),
            )
            .ok();
        if matches!(
            active_execution.as_deref(),
            Some("queued") | Some("running")
        ) {
            return Err("cannot delete an active agent session".to_string());
        }
        conn.execute(
            "DELETE FROM chat_sessions WHERE id = ?1",
            rusqlite::params![sid],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
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

const INTERRUPTED_TOOL_CALL_ERROR: &str =
    "Error: this tool call did not complete because the previous response was interrupted. Please retry with a shorter input, or break the work into smaller steps.";
const TOKEN_LIMIT_CONTINUE_PROMPT: &str =
    "Your response was cut off due to the token limit. Please continue where you left off. If you were writing a file, try breaking the content into smaller pieces.";

#[derive(Debug, Clone)]
struct LoadedChatState {
    agent_id: String,
    history: Vec<ChatMessage>,
    session_title: String,
    chain_depth: i64,
    session_type: String,
    execution_state: Option<String>,
    user_msg_id: String,
    user_msg_now: String,
    user_msg_content_json: String,
}

fn is_tool_result_message(message: &ChatMessage) -> bool {
    message.role == "user"
        && !message.content.is_empty()
        && message
            .content
            .iter()
            .all(|block| matches!(block, ContentBlock::ToolResult { .. }))
}

fn interrupted_tool_results_for_ids(tool_use_ids: &[String]) -> Vec<ContentBlock> {
    tool_use_ids
        .into_iter()
        .map(|tool_use_id| ContentBlock::ToolResult {
            tool_use_id: tool_use_id.clone(),
            content: INTERRUPTED_TOOL_CALL_ERROR.to_string(),
            is_error: true,
        })
        .collect()
}

fn sanitize_history_for_provider(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut sanitized: Vec<ChatMessage> = Vec::new();
    let mut index = 0usize;

    while index < messages.len() {
        let message = &messages[index];

        if is_tool_result_message(message) {
            index += 1;
            continue;
        }

        sanitized.push(message.clone());

        let tool_use_ids: Vec<String> = message
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolUse { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect();

        if tool_use_ids.is_empty() {
            index += 1;
            continue;
        }

        let mut combined_results: Vec<ContentBlock> = Vec::new();
        if let Some(next_message) = messages.get(index + 1) {
            if is_tool_result_message(next_message) {
                combined_results.extend(next_message.content.iter().filter_map(
                    |block| match block {
                        ContentBlock::ToolResult { tool_use_id, .. }
                            if tool_use_ids.contains(tool_use_id) =>
                        {
                            Some(block.clone())
                        }
                        _ => None,
                    },
                ));
                index += 1;
            }
        }

        let existing_ids: Vec<String> = combined_results
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.clone()),
                _ => None,
            })
            .collect();

        let missing_ids: Vec<String> = tool_use_ids
            .into_iter()
            .filter(|tool_use_id| !existing_ids.contains(tool_use_id))
            .collect();
        combined_results.extend(interrupted_tool_results_for_ids(&missing_ids));

        if !combined_results.is_empty() {
            sanitized.push(ChatMessage {
                role: "user".to_string(),
                content: combined_results,
                created_at: None,
            });
        }

        index += 1;
    }

    sanitized
}

#[tauri::command]
pub async fn get_chat_messages(
    session_id: String,
    limit: Option<i64>,
    offset: Option<i64>,
    db: tauri::State<'_, DbPool>,
) -> Result<PaginatedChatMessages, String> {
    let pool = db.0.clone();

    tokio::task::spawn_blocking(move || {
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
                    "SELECT id, role, content, created_at, is_compacted FROM (
               SELECT id, role, content, created_at, is_compacted
               FROM chat_messages WHERE session_id = ?1
               ORDER BY created_at DESC
               LIMIT ?2 OFFSET ?3
             ) sub ORDER BY created_at ASC",
                )
                .map_err(|e| e.to_string())?;

            let rows: Vec<ChatMessageWithMeta> = stmt
                .query_map(
                    rusqlite::params![session_id, limit_val, offset_val],
                    |row| {
                        let id: String = row.get(0)?;
                        let role: String = row.get(1)?;
                        let content_json: String = row.get(2)?;
                        let created_at: Option<String> = row.get(3)?;
                        let is_compacted: bool = row.get(4)?;
                        Ok((id, role, content_json, created_at, is_compacted))
                    },
                )
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .map(|(id, role, content_json, created_at, is_compacted)| {
                    let content: Vec<ContentBlock> =
                        serde_json::from_str(&content_json).unwrap_or_default();
                    ChatMessageWithMeta {
                        id: Some(id),
                        role,
                        content,
                        created_at,
                        is_compacted,
                    }
                })
                .collect();
            rows
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT id, role, content, created_at, is_compacted FROM chat_messages
                   WHERE session_id = ?1 ORDER BY created_at ASC",
                )
                .map_err(|e| e.to_string())?;

            let rows: Vec<ChatMessageWithMeta> = stmt
                .query_map(rusqlite::params![session_id], |row| {
                    let id: String = row.get(0)?;
                    let role: String = row.get(1)?;
                    let content_json: String = row.get(2)?;
                    let created_at: Option<String> = row.get(3)?;
                    let is_compacted: bool = row.get(4)?;
                    Ok((id, role, content_json, created_at, is_compacted))
                })
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .map(|(id, role, content_json, created_at, is_compacted)| {
                    let content: Vec<ContentBlock> =
                        serde_json::from_str(&content_json).unwrap_or_default();
                    ChatMessageWithMeta {
                        id: Some(id),
                        role,
                        content,
                        created_at,
                        is_compacted,
                    }
                })
                .collect();
            rows
        };

        let has_more = if limit_val > 0 {
            (offset_val + limit_val) < total_count
        } else {
            false
        };

        Ok(PaginatedChatMessages {
            messages,
            total_count,
            has_more,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

// ─── Send message (streaming) ───────────────────────────────────────────────

#[tauri::command]
pub async fn send_chat_message(
    session_id: String,
    content: String, // JSON-serialized Vec<ContentBlock>
    model_override: Option<ChatModelOverride>,
    app: tauri::AppHandle,
    db: tauri::State<'_, DbPool>,
    executor_tx: tauri::State<'_, ExecutorTx>,
    agent_semaphores: tauri::State<'_, AgentSemaphores>,
    session_registry: tauri::State<'_, SessionExecutionRegistry>,
    permission_registry: tauri::State<'_, PermissionRegistry>,
    user_question_registry: tauri::State<'_, UserQuestionRegistry>,
    memory_state: tauri::State<'_, Option<MemoryServiceState>>,
    auth: tauri::State<'_, AuthState>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<SendChatMessageResponse, String> {
    let pool = db.0.clone();
    let stream_id = format!("chat:{}", session_id);
    let stream_id_ret = stream_id.clone();

    // Parse user content blocks
    let user_content: Vec<ContentBlock> =
        serde_json::from_str(&content).map_err(|e| format!("invalid content: {}", e))?;

    // Grab the cloud client before the blocking task so we can sync the user message afterwards
    let cloud_client = cloud.get();

    // Load session + history in blocking task
    let loaded = {
        let pool = pool.clone();
        let sid = session_id.clone();
        let uc = user_content.clone();

        tokio::task::spawn_blocking(move || -> Result<LoadedChatState, String> {
            let conn = pool.get().map_err(|e| e.to_string())?;

            // Get session
            let (agent_id, title, chain_depth, session_type, execution_state): (String, String, i64, String, Option<String>) = conn
            .query_row(
              "SELECT agent_id, title, chain_depth, session_type, execution_state FROM chat_sessions WHERE id = ?1",
              rusqlite::params![sid],
              |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
            )
            .map_err(|e| format!("session not found: {}", e))?;

            // Load existing messages (exclude compacted ones — only active context goes to LLM)
            let mut stmt = conn
                .prepare(
                    "SELECT role, content FROM chat_messages
                     WHERE session_id = ?1 AND is_compacted = 0 ORDER BY created_at ASC",
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
                    let content: Vec<ContentBlock> =
                        serde_json::from_str(&content_json).unwrap_or_default();
                    ChatMessage {
                        role,
                        content,
                        created_at: None,
                    }
                })
                .collect();

            messages = sanitize_history_for_provider(&messages);

            // Save user message to DB
            let msg_id = Ulid::new().to_string();
            let now = chrono::Utc::now().to_rfc3339();
            let content_json = serde_json::to_string(&uc).map_err(|e| e.to_string())?;

            conn.execute(
                "INSERT INTO chat_messages (id, session_id, role, content, created_at)
                 VALUES (?1, ?2, 'user', ?3, ?4)",
                rusqlite::params![msg_id, sid, content_json, now],
            )
            .map_err(|e| e.to_string())?;

            // Update session timestamp
            conn.execute(
                "UPDATE chat_sessions SET updated_at = ?1 WHERE id = ?2",
                rusqlite::params![now, sid],
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
                        rusqlite::params![t, sid],
                    );
                }
            }

            // Append user message to history
            messages.push(ChatMessage {
                role: "user".to_string(),
                content: uc,
                created_at: None,
            });

            Ok(LoadedChatState {
                agent_id,
                history: messages,
                session_title: title,
                chain_depth,
                session_type,
                execution_state,
                user_msg_id: msg_id,
                user_msg_now: now,
                user_msg_content_json: content_json,
            })
        })
        .await
        .map_err(|e| e.to_string())??
    };

    let LoadedChatState {
        agent_id,
        history,
        session_title: _session_title,
        chain_depth,
        session_type,
        execution_state,
        user_msg_id,
        user_msg_now,
        user_msg_content_json,
    } = loaded;

    let db_bg = DbPool(pool.clone());

    // Sync the initial user message to Supabase (was missing — only SQLite was written above)
    if let Some(client) = cloud_client.clone() {
        let sid_cloud = session_id.clone();
        let msg_id_cloud = user_msg_id.clone();
        let now_cloud = user_msg_now.clone();
        let content_json_cloud = user_msg_content_json.clone();
        tokio::spawn(async move {
            if let Err(e) = client
                .upsert_chat_message(
                    &msg_id_cloud,
                    &sid_cloud,
                    "user",
                    &content_json_cloud,
                    &now_cloud,
                )
                .await
            {
                warn!("cloud upsert initial user message: {}", e);
            }
        });
    }

    let is_waiting_for_message = execution_state.as_deref() == Some("waiting_message");
    if is_waiting_for_message {
        return Ok(SendChatMessageResponse {
            stream_id: stream_id_ret,
            user_message_id: user_msg_id,
        });
    }

    session_registry.clear_cancelled(&session_id).await;

    if let Err(err) =
        session_agent::update_session_execution_state(&db_bg, &session_id, "running", None, None)
            .await
    {
        warn!(
            session_id = %session_id,
            "failed to mark chat session as running: {}",
            err
        );
    }

    // Resolve memory user_id from auth state
    let memory_user_id = match auth.get().await {
        AuthMode::Cloud(session) => session.user_id,
        _ => "default_user".to_string(),
    };

    // Spawn the LLM call on a background task so the command returns immediately
    let sid_bg = session_id.clone();
    let session_type_bg = session_type.clone();
    let etx = executor_tx.0.clone();
    let semaphores = agent_semaphores.inner().clone();
    let registry = session_registry.inner().clone();
    let perm_registry = permission_registry.inner().clone();
    let question_registry = user_question_registry.inner().clone();
    let mem_client = memory_state.as_ref().map(|s| s.client.clone());
    let model_override_bg = model_override.clone();

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
            &session_type_bg,
            semaphores,
            registry,
            perm_registry,
            question_registry,
            mem_client.as_ref(),
            &memory_user_id,
            model_override_bg.as_ref(),
            cloud_client.clone(),
        )
        .await
        {
            warn!("Chat LLM error: {}", e);
            if e == "cancelled" {
                let _ = session_agent::finalize_cancelled_session(&db_bg, &sid_bg).await;
            } else {
                let _ = session_agent::finalize_failed_session(&db_bg, &sid_bg, &e).await;
                let error_message = vec![ContentBlock::Text {
                    text: format!(
                        "I ran into an error while continuing this chat.\n\n{}\n\nIf this happened after a large tool call, please retry with a smaller request or ask me to split the work into smaller steps.",
                        e
                    ),
                }];
                if let Err(save_err) = save_chat_message(
                    &db_bg.0,
                    &sid_bg,
                    "assistant",
                    &error_message,
                    cloud_client.clone(),
                )
                .await
                {
                    warn!("failed to persist chat error message: {}", save_err);
                }
            }
            // Emit finished with error info
            emit_agent_iteration(&app, &stream_id, 1, "finished", None, 0, None);
        }
    });

    Ok(SendChatMessageResponse {
        stream_id: stream_id_ret,
        user_message_id: user_msg_id,
    })
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
        )
        .map_err(|e| e.to_string())?;

        conn.execute(
            "UPDATE chat_sessions SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now, sid],
        )
        .map_err(|e| e.to_string())?;

        Ok((msg_id, now))
    })
    .await
    .map_err(|e| e.to_string())??;

    if let Some(client) = cloud {
        tokio::spawn(async move {
            if let Err(e) = client
                .upsert_chat_message(&msg_id, &sid_clone, &role_clone, &content_json_clone, &now)
                .await
            {
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
    session_type: &str,
    agent_semaphores: AgentSemaphores,
    session_registry: SessionExecutionRegistry,
    permission_registry: PermissionRegistry,
    user_question_registry: UserQuestionRegistry,
    memory_client: Option<&MemoryClient>,
    memory_user_id: &str,
    model_override: Option<&ChatModelOverride>,
    cloud_client: Option<std::sync::Arc<crate::db::cloud::SupabaseClient>>,
) -> Result<(), String> {
    // Load agent config
    let mut ws_config = workspace::load_agent_config(agent_id).unwrap_or_default();
    if let Some(model_override) = model_override {
        ws_config.provider = model_override.provider.clone();
        ws_config.model = model_override.model.clone();
    }

    let provider_name = &ws_config.provider;
    let api_key = keychain::retrieve_api_key(provider_name)
        .map_err(|_| format!("No API key for provider '{}'", provider_name))?;

    if let Some(latest_user_blocks) = messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| message.content.as_slice())
    {
        let activated_skill_names = activate_skill_mentions_for_session(
            db,
            session_id,
            agent_id,
            &ws_config.disabled_skills,
            latest_user_blocks,
        )?;
        if !activated_skill_names.is_empty() {
            info!(
                session_id = session_id,
                skills = ?activated_skill_names,
                "activated skills from chat mentions"
            );
        }
    }

    let chat_project_id = session_worktree::load_session_project_id(db, session_id).await?;
    if let Some(pid) = chat_project_id.as_deref() {
        crate::commands::projects::assert_agent_in_project(db, pid, agent_id).await?;
        if let Err(e) = workspace::init_project_workspace(pid) {
            warn!(project_id = pid, "failed to init project workspace: {}", e);
        }
    }

    // Build context via pipeline (messages already loaded, pass them to avoid re-query)
    let pipeline = context::default_pipeline(memory_client.cloned());
    let allowed_tools = ContextRequest::effective_allowed_tools(&ws_config);
    let ctx_request = ContextRequest {
        agent_id: agent_id.to_string(),
        mode: ContextMode::Chat,
        session_id: Some(session_id.to_string()),
        session_type: Some(session_type.to_string()),
        project_id: chat_project_id.clone(),
        goal: None,
        ws_config: ws_config.clone(),
        allowed_tools,
        existing_messages: Some(messages),
        is_sub_agent: false,
        allow_sub_agents: true,
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

    let chat_worktree = crate::executor::tools::context::sanitize_session_worktree_state(
        db,
        session_id,
        agent_id,
        chat_project_id.as_deref(),
        session_worktree::load_session_worktree_state(db, session_id).await?,
        cloud_client.clone(),
    )
    .await?;
    let tool_ctx = ToolExecutionContext::new_with_bus(
        agent_id,
        stream_id,
        Some(session_id),
        chain_depth,
        db.clone(),
        executor_tx.clone(),
        app.clone(),
        agent_semaphores,
        session_registry.clone(),
        chat_worktree,
        chat_project_id.as_deref(),
    )
    .with_permission_registry(permission_registry.clone())
    .with_user_question_registry(user_question_registry)
    .with_memory_client(memory_client.cloned())
    .with_memory_user_id(memory_user_id.to_string())
    .with_cloud_client(cloud_client.clone());
    let tool_ctx = std::sync::Arc::new(tool_ctx);

    // ── Create provider (wiring MCP bridge for CLI providers) ────────────
    let mcp_handle: Option<crate::executor::mcp_server::McpServerHandle> = app
        .try_state::<crate::executor::mcp_server::McpServerHandle>()
        .map(|s| s.inner().clone());
    let wiring = mcp_handle.map(|handle| crate::executor::llm_provider::AgentMcpWiring {
        handle,
        agent_id: agent_id.to_string(),
        run_id: stream_id.to_string(),
        tool_ctx: tool_ctx.clone(),
        tools: tools.clone(),
        permission_registry: permission_registry.clone(),
        app: app.clone(),
        db: db.clone(),
    });
    let provider = llm_provider::create_provider_with_mcp(provider_name, api_key, wiring)?;

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

        if is_chat_session_cancelled(session_id, db, &session_registry).await {
            return Err("cancelled".to_string());
        }

        debug!(
            session_id = session_id,
            message_count = messages.len(),
            iteration = iteration,
            "Chat LLM call (iteration {})",
            iteration,
        );

        emit_agent_iteration(
            app,
            stream_id,
            iteration,
            "llm_call",
            None,
            cumulative_input_tokens + cumulative_output_tokens,
            None,
        );

        let response = chat_streaming_with_cancellation(
            provider.as_ref(),
            &config,
            &messages,
            &tools,
            app,
            stream_id,
            iteration,
            session_id,
            db,
            &session_registry,
        )
        .await?;

        cumulative_input_tokens += response.usage.input_tokens;
        cumulative_output_tokens += response.usage.output_tokens;

        if is_chat_session_cancelled(session_id, db, &session_registry).await {
            return Err("cancelled".to_string());
        }

        // Save assistant response to DB
        save_chat_message(
            &pool,
            session_id,
            "assistant",
            &response.content,
            cloud_client.clone(),
        )
        .await?;

        match response.stop_reason {
            llm_provider::StopReason::EndTurn => {
                messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: response.content,
                    created_at: None,
                });
                break;
            }

            llm_provider::StopReason::MaxTokens => {
                messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: response.content.clone(),
                    created_at: None,
                });

                let mut tool_error_results: Vec<ContentBlock> = Vec::new();
                for block in &response.content {
                    if let ContentBlock::ToolUse { id, name, .. } = block {
                        warn!(
                            session_id = session_id,
                            tool = %name,
                            "chat tool_use truncated by max_tokens"
                        );
                        tool_error_results.push(ContentBlock::ToolResult {
                            tool_use_id: id.clone(),
                            content: INTERRUPTED_TOOL_CALL_ERROR.to_string(),
                            is_error: true,
                        });
                        emit_agent_tool_result(
                            app,
                            stream_id,
                            iteration,
                            id,
                            INTERRUPTED_TOOL_CALL_ERROR,
                            true,
                        );
                    }
                }

                if !tool_error_results.is_empty() {
                    save_chat_message(
                        &pool,
                        session_id,
                        "user",
                        &tool_error_results,
                        cloud_client.clone(),
                    )
                    .await?;

                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: tool_error_results,
                        created_at: None,
                    });
                }

                messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: vec![ContentBlock::Text {
                        text: TOKEN_LIMIT_CONTINUE_PROMPT.to_string(),
                    }],
                    created_at: None,
                });
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
                        if is_chat_session_cancelled(session_id, db, &session_registry).await {
                            return Err("cancelled".to_string());
                        }

                        emit_agent_iteration(
                            app,
                            stream_id,
                            iteration,
                            "tool_exec",
                            Some(name),
                            cumulative_input_tokens + cumulative_output_tokens,
                            None,
                        );

                        match permissions::execute_tool_with_permissions(
                            tool_ctx.as_ref(),
                            name,
                            input,
                            app,
                            stream_id,
                            &permission_registry,
                        )
                        .await
                        {
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
                                emit_agent_tool_result(
                                    app, stream_id, iteration, id, &result, false,
                                );
                            }
                            Err(err) => {
                                let err_content = format!("Error: {}", err);
                                tool_results.push(ContentBlock::ToolResult {
                                    tool_use_id: id.clone(),
                                    content: err_content.clone(),
                                    is_error: true,
                                });
                                emit_agent_tool_result(
                                    app,
                                    stream_id,
                                    iteration,
                                    id,
                                    &err_content,
                                    true,
                                );
                            }
                        }

                        if is_chat_session_cancelled(session_id, db, &session_registry).await {
                            return Err("cancelled".to_string());
                        }
                    }
                }

                // Save tool results to DB and add to conversation
                save_chat_message(
                    &pool,
                    session_id,
                    "user",
                    &tool_results,
                    cloud_client.clone(),
                )
                .await?;

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
    emit_agent_iteration(
        app,
        stream_id,
        iteration,
        "finished",
        None,
        total_tokens,
        None,
    );

    // Emit context window usage update
    emit_chat_context_update(
        app,
        session_id,
        cumulative_input_tokens,
        cumulative_output_tokens,
        context_window,
    );

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
        })
        .await;
    }

    info!(
        session_id = session_id,
        "Chat complete ({} tokens, {} iterations)", total_tokens, iteration
    );

    if let Err(err) =
        session_agent::update_session_execution_state(db, session_id, "success", None, None).await
    {
        warn!(
            session_id = %session_id,
            "failed to mark chat session as successful: {}",
            err
        );
    }

    // Check if compaction is needed
    let threshold = compaction::effective_threshold(&ws_config);
    if compaction::should_compact(cumulative_input_tokens, context_window, threshold) {
        // Circuit breaker: skip auto-compaction if too many recent failures
        let db_check = DbPool(db.0.clone());
        let circuit_open = compaction::is_circuit_open(&db_check, session_id).unwrap_or(false);
        if circuit_open {
            warn!(
                session_id = session_id,
                "Auto-compaction skipped: circuit breaker open after repeated failures"
            );
        } else {
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
            let compaction_user_id = memory_user_id.to_string();
            let compact_memory_client = memory_client.cloned();
            let compact_cloud_client = cloud_client.clone();
            match keychain::retrieve_api_key(provider_name) {
                Ok(compact_api_key) => {
                    match llm_provider::create_provider(provider_name, compact_api_key) {
                        Ok(compact_provider) => {
                            tauri::async_runtime::spawn(async move {
                                match compaction::perform_compaction(
                                    &agent_id,
                                    &session_id,
                                    compact_provider.as_ref(),
                                    &ws_config,
                                    &app,
                                    &db,
                                    compact_memory_client,
                                    &compaction_user_id,
                                    compact_cloud_client,
                                )
                                .await
                                {
                                    Ok(compaction::CompactionOutcome::Performed) => {
                                        info!(session_id = %session_id, "Background compaction completed")
                                    }
                                    Ok(compaction::CompactionOutcome::Skipped(reason)) => {
                                        info!(session_id = %session_id, "Background compaction skipped: {}", reason)
                                    }
                                    Err(e) => {
                                        warn!(session_id = %session_id, "Background compaction failed: {}", e);
                                        let _ =
                                            compaction::record_compaction_failure(&db, &session_id);
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            warn!(session_id = %session_id, "Background compaction provider setup failed: {}", e);
                            let _ = compaction::record_compaction_failure(&db, &session_id);
                        }
                    }
                }
                Err(_) => {
                    warn!(session_id = %session_id, "Background compaction skipped: no API key for provider '{}'", provider_name);
                    let _ = compaction::record_compaction_failure(&db, &session_id);
                }
            }
        }
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSessionMeta {
    pub session_id: String,
    pub agent_id: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
}

#[tauri::command]
pub async fn get_chat_session_meta(
    session_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<ChatSessionMeta, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let sid = session_id.clone();
        conn.query_row(
            "SELECT cs.agent_id, cs.project_id, p.name
             FROM chat_sessions cs
             LEFT JOIN projects p ON p.id = cs.project_id
             WHERE cs.id = ?1",
            rusqlite::params![session_id],
            |row| {
                Ok(ChatSessionMeta {
                    session_id: sid.clone(),
                    agent_id: row.get(0)?,
                    project_id: row.get(1)?,
                    project_name: row.get(2)?,
                })
            },
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_session_execution(
    session_id: String,
    db: tauri::State<'_, DbPool>,
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
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn cancel_agent_session(
    session_id: String,
    db: tauri::State<'_, DbPool>,
    session_registry: tauri::State<'_, SessionExecutionRegistry>,
    user_question_registry: tauri::State<'_, UserQuestionRegistry>,
) -> Result<(), String> {
    let pool = db.0.clone();
    let sid = session_id.clone();
    let session_type: String = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT session_type FROM chat_sessions WHERE id = ?1",
            rusqlite::params![sid],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    if !can_cancel_chat_session(&session_type) {
        return Err(
            "only user_chat, bus_message, sub_agent, and pulse sessions can be cancelled"
                .to_string(),
        );
    }

    session_registry.cancel(&session_id).await;
    user_question_registry.cancel_for_session(&session_id).await;
    let db_pool = DbPool(db.0.clone());
    session_agent::update_session_execution_state(
        &db_pool,
        &session_id,
        "cancelled",
        None,
        Some("Cancelled".to_string()),
    )
    .await
}

#[tauri::command]
pub async fn respond_to_user_question(
    request_id: String,
    response: String,
    registry: tauri::State<'_, UserQuestionRegistry>,
) -> Result<(), String> {
    registry.resolve(&request_id, response).await
}

// ─── Context Usage Query ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    pub input_tokens: u32,
    pub context_window_size: u32,
    pub usage_percent: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatModelOverride {
    pub provider: String,
    pub model: String,
}

#[tauri::command]
pub async fn get_context_usage(
    session_id: String,
    model_override: Option<ChatModelOverride>,
    db: tauri::State<'_, DbPool>,
) -> Result<ContextUsage, String> {
    let pool = db.0.clone();

    let (last_input_tokens, agent_id) = tokio::task::spawn_blocking({
        let pool = pool.clone();
        let sid = session_id.clone();
        move || -> Result<(Option<u32>, String), String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            let row: (Option<u32>, String) = conn
                .query_row(
                    "SELECT last_input_tokens, agent_id FROM chat_sessions WHERE id = ?1",
                    rusqlite::params![sid],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|e| format!("session not found: {}", e))?;
            Ok(row)
        }
    })
    .await
    .map_err(|e| e.to_string())??;

    let mut ws_config = workspace::load_agent_config(&agent_id).unwrap_or_default();
    if let Some(model_override) = model_override {
        ws_config.provider = model_override.provider;
        ws_config.model = model_override.model;
    }
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
    cloud: tauri::State<'_, CloudClientState>,
    memory_state: tauri::State<'_, Option<MemoryServiceState>>,
) -> Result<(), String> {
    let pool = db.0.clone();

    // Look up agent_id for this session
    let agent_id: String = tokio::task::spawn_blocking({
        let pool = pool.clone();
        let sid = session_id.clone();
        move || -> Result<String, String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            conn.query_row(
                "SELECT agent_id FROM chat_sessions WHERE id = ?1",
                rusqlite::params![sid],
                |row| row.get(0),
            )
            .map_err(|e| format!("session not found: {}", e))
        }
    })
    .await
    .map_err(|e| e.to_string())??;

    let ws_config = workspace::load_agent_config(&agent_id).unwrap_or_default();
    let provider_name = &ws_config.provider;
    let api_key = keychain::retrieve_api_key(provider_name)
        .map_err(|_| format!("No API key for provider '{}'", provider_name))?;
    let provider = llm_provider::create_provider(provider_name, api_key)?;

    let memory_user_id = match auth.get().await {
        AuthMode::Cloud(session) => session.user_id,
        _ => "default_user".to_string(),
    };

    // Manual compaction bypasses the circuit breaker intentionally
    let mem_client = memory_state.as_ref().map(|s| s.client.clone());
    let cloud_client = cloud.get();
    let db_pool = DbPool(pool);
    let outcome = compaction::perform_compaction(
        &agent_id,
        &session_id,
        provider.as_ref(),
        &ws_config,
        &app,
        &db_pool,
        mem_client,
        &memory_user_id,
        cloud_client,
    )
    .await?;

    match outcome {
        compaction::CompactionOutcome::Performed => {
            info!(session_id = %session_id, "Manual compaction completed");
        }
        compaction::CompactionOutcome::Skipped(reason) => {
            info!(session_id = %session_id, "Manual compaction skipped: {}", reason);
        }
    }
    Ok(())
}

// ─── Message reactions ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageReactionRow {
    pub id: String,
    pub message_id: String,
    pub emoji: String,
    pub created_at: String,
}

#[tauri::command]
pub async fn get_message_reactions(
    session_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<MessageReactionRow>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, message_id, emoji, created_at FROM message_reactions
                 WHERE session_id = ?1 ORDER BY created_at ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows: Vec<MessageReactionRow> = stmt
            .query_map(rusqlite::params![session_id], |row| {
                Ok(MessageReactionRow {
                    id: row.get(0)?,
                    message_id: row.get(1)?,
                    emoji: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ─── SendChatMessage response ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendChatMessageResponse {
    pub stream_id: String,
    pub user_message_id: String,
}

#[cfg(test)]
mod tests {
    use super::{
        can_cancel_chat_session, sanitize_history_for_provider, ChatMessage, ContentBlock,
        INTERRUPTED_TOOL_CALL_ERROR,
    };

    #[test]
    fn sanitize_history_reinserts_missing_tool_results_after_tool_use() {
        let messages = vec![
            ChatMessage {
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::ToolUse {
                        id: "tool-1".to_string(),
                        name: "write_file".to_string(),
                        input: serde_json::json!({ "path": "a.txt" }),
                    },
                    ContentBlock::ToolUse {
                        id: "tool-2".to_string(),
                        name: "write_file".to_string(),
                        input: serde_json::json!({ "path": "b.txt" }),
                    },
                ],
                created_at: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: "Can you keep going?".to_string(),
                }],
                created_at: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tool-2".to_string(),
                    content: INTERRUPTED_TOOL_CALL_ERROR.to_string(),
                    is_error: true,
                }],
                created_at: None,
            },
        ];

        let sanitized = sanitize_history_for_provider(&messages);

        assert_eq!(sanitized.len(), 3);
        assert!(matches!(
            &sanitized[1],
            ChatMessage { role, content, .. }
                if role == "user"
                    && matches!(
                        &content[0],
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error: true,
                        } if tool_use_id == "tool-1" && content == INTERRUPTED_TOOL_CALL_ERROR
                    )
                    && matches!(
                        &content[1],
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error: true,
                        } if tool_use_id == "tool-2" && content == INTERRUPTED_TOOL_CALL_ERROR
                    )
        ));
    }

    #[test]
    fn sanitize_history_keeps_consistent_history_unchanged() {
        let messages = vec![ChatMessage {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: "All done".to_string(),
            }],
            created_at: None,
        }];

        let sanitized = sanitize_history_for_provider(&messages);
        assert_eq!(sanitized.len(), 1);
        assert!(matches!(
            &sanitized[0].content[0],
            ContentBlock::Text { text } if text == "All done"
        ));
    }

    #[test]
    fn user_chat_sessions_can_be_cancelled() {
        assert!(can_cancel_chat_session("user_chat"));
        assert!(can_cancel_chat_session("bus_message"));
        assert!(can_cancel_chat_session("sub_agent"));
        assert!(can_cancel_chat_session("pulse"));
        assert!(!can_cancel_chat_session("unknown"));
    }
}
