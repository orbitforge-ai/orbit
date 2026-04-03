use tracing::{debug, info, warn};
use ulid::Ulid;

use crate::db::connection::DbPool;
use crate::events::emitter::emit_chat_context_update;
use crate::executor::llm_provider::{
    model_context_window, ChatMessage, ContentBlock, LlmConfig, LlmProvider,
};
use crate::executor::memory::MemoryClient;
use crate::executor::workspace::AgentWorkspaceConfig;

const DEFAULT_COMPACTION_THRESHOLD: f64 = 0.65;
const DEFAULT_COMPACTION_RETAIN_COUNT: u32 = 12;
const COMPACTION_MAX_TOKENS: u32 = 1024;

const COMPACTION_SYSTEM_PROMPT: &str = r#"You are a conversation summarizer. Summarize the following conversation into a concise but comprehensive summary. Preserve:
- Key facts, decisions, and conclusions
- Code snippets, file paths, and technical details that were discussed
- Any pending questions or tasks
- The user's goals and preferences expressed during the conversation

Be thorough but concise. The summary will replace these messages in the conversation context, so include everything needed to continue the conversation coherently."#;

/// Returns true if the current context usage exceeds the compaction threshold.
pub fn should_compact(input_tokens: u32, context_window: u32, threshold: f64) -> bool {
    if context_window == 0 {
        return false;
    }
    let usage = input_tokens as f64 / context_window as f64;
    usage >= threshold
}

/// Resolves the effective context window size for an agent config.
pub fn effective_context_window(config: &AgentWorkspaceConfig) -> u32 {
    config
        .context_window_override
        .unwrap_or_else(|| model_context_window(&config.model))
}

/// Resolves the compaction threshold (0.0–1.0) from agent config or default.
pub fn effective_threshold(config: &AgentWorkspaceConfig) -> f64 {
    config
        .compaction_threshold
        .unwrap_or(DEFAULT_COMPACTION_THRESHOLD)
}

/// Resolves the number of recent messages to retain during compaction.
pub fn effective_retain_count(config: &AgentWorkspaceConfig) -> u32 {
    config
        .compaction_retain_count
        .unwrap_or(DEFAULT_COMPACTION_RETAIN_COUNT)
}

/// Splits messages into (to_compact, to_keep).
/// `to_keep` is the last `retain_count` messages; `to_compact` is everything before that.
/// Returns None if there aren't enough messages to compact.
pub fn select_messages_for_compaction(
    messages: &[ChatMessage],
    retain_count: u32,
) -> Option<(Vec<ChatMessage>, Vec<ChatMessage>)> {
    let retain = retain_count as usize;
    if messages.len() <= retain {
        return None;
    }
    let split_point = messages.len() - retain;
    let to_compact = messages[..split_point].to_vec();
    let to_keep = messages[split_point..].to_vec();

    if to_compact.is_empty() {
        return None;
    }

    Some((to_compact, to_keep))
}

/// Performs the full compaction flow for a chat session.
pub async fn perform_compaction(
    agent_id: &str,
    session_id: &str,
    provider: &dyn LlmProvider,
    ws_config: &AgentWorkspaceConfig,
    app: &tauri::AppHandle,
    db: &DbPool,
    memory_client: Option<MemoryClient>,
    memory_user_id: &str,
    cloud_client: Option<std::sync::Arc<crate::db::cloud::SupabaseClient>>,
) -> Result<(), String> {
    let pool = db.0.clone();
    let sid = session_id.to_string();

    // 1. Load non-compacted messages
    let messages = {
        let pool = pool.clone();
        let sid = sid.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<(String, ChatMessage)>, String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            let mut stmt = conn
                .prepare(
                    "SELECT id, role, content FROM chat_messages
                     WHERE session_id = ?1 AND is_compacted = 0
                     ORDER BY created_at ASC",
                )
                .map_err(|e| e.to_string())?;

            let msgs = stmt
                .query_map(rusqlite::params![sid], |row| {
                    let id: String = row.get(0)?;
                    let role: String = row.get(1)?;
                    let content_json: String = row.get(2)?;
                    Ok((id, role, content_json))
                })
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .map(|(id, role, content_json)| {
                    let content: Vec<ContentBlock> =
                        serde_json::from_str(&content_json).unwrap_or_default();
                    (
                        id,
                        ChatMessage {
                            role,
                            content,
                            created_at: None,
                        },
                    )
                })
                .collect();

            Ok(msgs)
        })
        .await
        .map_err(|e| e.to_string())??
    };

    let retain_count = effective_retain_count(ws_config);
    let msg_ids: Vec<String> = messages.iter().map(|(id, _)| id.clone()).collect();
    let chat_messages: Vec<ChatMessage> = messages.into_iter().map(|(_, msg)| msg).collect();

    // 2. Determine what to compact
    let (to_compact, _to_keep) = match select_messages_for_compaction(&chat_messages, retain_count)
    {
        Some(split) => split,
        None => {
            debug!("Not enough messages to compact for session {}", session_id);
            return Ok(());
        }
    };

    let compact_count = to_compact.len();
    let compacted_msg_ids: Vec<String> = msg_ids[..compact_count].to_vec();

    info!(
        session_id = session_id,
        "Compacting {} messages, retaining {}",
        compact_count,
        chat_messages.len() - compact_count
    );

    // 3. Generate summary via LLM
    let summary_config = LlmConfig {
        model: ws_config.model.clone(),
        max_tokens: COMPACTION_MAX_TOKENS,
        temperature: Some(ws_config.temperature),
        system_prompt: COMPACTION_SYSTEM_PROMPT.to_string(),
    };

    let mut conversation_text = String::new();
    for msg in &to_compact {
        let role_label = if msg.role == "user" {
            "User"
        } else {
            "Assistant"
        };
        for block in &msg.content {
            match block {
                ContentBlock::Text { text } => {
                    conversation_text.push_str(&format!("{}: {}\n\n", role_label, text));
                }
                ContentBlock::Thinking { thinking } => {
                    conversation_text
                        .push_str(&format!("{} (thinking): {}\n\n", role_label, thinking));
                }
                ContentBlock::ToolUse { name, input, .. } => {
                    conversation_text.push_str(&format!(
                        "{} used tool `{}` with input: {}\n\n",
                        role_label, name, input
                    ));
                }
                ContentBlock::ToolResult { content, .. } => {
                    conversation_text.push_str(&format!("Tool result: {}\n\n", content));
                }
                ContentBlock::Image { .. } => {
                    conversation_text.push_str(&format!("{}: [image]\n\n", role_label));
                }
            }
        }
    }

    let summary_request = vec![ChatMessage {
        role: "user".to_string(),
        content: vec![ContentBlock::Text {
            text: format!(
                "Please summarize the following conversation:\n\n{}",
                conversation_text
            ),
        }],
        created_at: None,
    }];

    let response = provider
        .chat_streaming(
            &summary_config,
            &summary_request,
            &[],
            app,
            "compaction:internal",
            0,
        )
        .await?;

    // Extract summary text from response
    let summary_text = response
        .content
        .iter()
        .filter_map(|block| {
            if let ContentBlock::Text { text } = block {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<&str>>()
        .join("\n");

    if summary_text.is_empty() {
        return Err("Compaction LLM returned empty summary".to_string());
    }

    let summary_content = format!("[Conversation Summary]\n{}", summary_text);
    let summary_token_count = response.usage.input_tokens + response.usage.output_tokens;

    // 4. Persist: insert summary message, flag compacted messages, record compaction
    let pool = db.0.clone();
    let sid = sid.clone();
    let compacted_ids_json =
        serde_json::to_string(&compacted_msg_ids).map_err(|e| e.to_string())?;
    let summary_msg_id = Ulid::new().to_string();
    let compaction_id = Ulid::new().to_string();

    // Clone before the closure so they remain available for cloud sync below.
    let compaction_id_post = compaction_id.clone();
    let summary_msg_id_post = summary_msg_id.clone();
    let compacted_ids_json_post = compacted_ids_json.clone();

    let original_token_count = compact_count as u32; // approximate; we don't have per-message counts yet

    let estimated_tokens = tokio::task::spawn_blocking(move || -> Result<u32, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();

        // Insert summary message (as assistant, placed just before the retained messages)
        let summary_content_json = serde_json::to_string(&vec![ContentBlock::Text {
            text: summary_content,
        }])
        .map_err(|e| e.to_string())?;

        conn.execute(
            "INSERT INTO chat_messages (id, session_id, role, content, created_at, token_count, is_compacted)
             VALUES (?1, ?2, 'assistant', ?3, ?4, ?5, 0)",
            rusqlite::params![summary_msg_id, sid, summary_content_json, now, summary_token_count],
        )
        .map_err(|e| e.to_string())?;

        // Flag original messages as compacted
        let placeholders: Vec<String> = compacted_msg_ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect();
        let sql = format!(
            "UPDATE chat_messages SET is_compacted = 1 WHERE id IN ({})",
            placeholders.join(", ")
        );
        let params: Vec<Box<dyn rusqlite::types::ToSql>> = compacted_msg_ids
            .iter()
            .map(|id| Box::new(id.clone()) as Box<dyn rusqlite::types::ToSql>)
            .collect();
        conn.execute(&sql, rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())))
            .map_err(|e| e.to_string())?;

        // Record compaction
        conn.execute(
            "INSERT INTO chat_compaction_summaries (id, session_id, summary_message_id, compacted_message_ids, original_token_count, summary_token_count, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                compaction_id,
                sid,
                summary_msg_id,
                compacted_ids_json,
                original_token_count,
                summary_token_count,
                now
            ],
        )
        .map_err(|e| e.to_string())?;

        // Estimate new context size from remaining non-compacted messages (~4 chars/token)
        let estimated_tokens: u32 = conn
            .query_row(
                "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM chat_messages
                 WHERE session_id = ?1 AND is_compacted = 0",
                rusqlite::params![sid],
                |row| row.get::<_, u32>(0),
            )
            .unwrap_or(0)
            / 4;

        conn.execute(
            "UPDATE chat_sessions SET last_input_tokens = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![estimated_tokens, now, sid],
        )
        .map_err(|e| e.to_string())?;

        Ok(estimated_tokens)
    })
    .await
    .map_err(|e| e.to_string())??;

    // Cloud sync: compaction summary
    if let Some(client) = &cloud_client {
      let client = client.clone();
      let uid = client.user_id.clone();
      let cid = compaction_id_post;
      let sid2 = session_id.to_string();
      let smid = summary_msg_id_post;
      let cids = compacted_ids_json_post;
      let otc = original_token_count;
      let stc = summary_token_count;
      let now = chrono::Utc::now().to_rfc3339();
      tokio::spawn(async move {
        let _ = client.upsert_chat_compaction_summary_json(serde_json::json!({
          "user_id": uid, "id": cid, "session_id": sid2,
          "summary_message_id": smid, "compacted_message_ids": cids,
          "original_token_count": otc, "summary_token_count": stc,
          "created_at": now,
        })).await;
      });
    }

    // 5. Emit updated context info
    let context_window = effective_context_window(ws_config);

    info!(
        session_id = session_id,
        "Compaction complete: {} messages compacted into summary, estimated {} tokens remaining",
        compact_count,
        estimated_tokens
    );

    emit_chat_context_update(app, session_id, estimated_tokens, 0, context_window);

    // Post-compaction memory extraction from the summary
    if ws_config.memory_enabled {
        if let Some(client) = memory_client {
            let agent_id = agent_id.to_string();
            let session_id = session_id.to_string();
            let user_id = memory_user_id.to_string();
            let db_clone = DbPool(db.0.clone());
            let extract_text = summary_text.clone();
            let cloud_cl = cloud_client.clone();
            tauri::async_runtime::spawn(async move {
                let log_id = Ulid::new().to_string();
                let now = chrono::Utc::now().to_rfc3339();
                {
                    let pool = db_clone.0.clone();
                    let log_id = log_id.clone();
                    let sid = session_id.clone();
                    let aid = agent_id.clone();
                    let now = now.clone();
                    let cloud_cl2 = cloud_cl.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        if let Ok(conn) = pool.get() {
                            let _ = conn.execute(
                                "INSERT INTO memory_extraction_log (id, session_id, agent_id, memories_extracted, status, created_at)
                                 VALUES (?1, ?2, ?3, 0, 'running', ?4)",
                                rusqlite::params![log_id, sid, aid, now],
                            );
                        }
                        if let Some(cl) = cloud_cl2 {
                            let uid = cl.user_id.clone();
                            tokio::spawn(async move {
                                let _ = cl.upsert_memory_extraction_log_json(serde_json::json!({
                                    "user_id": uid, "id": log_id, "session_id": sid,
                                    "agent_id": aid, "memories_extracted": 0,
                                    "status": "running", "created_at": now,
                                })).await;
                            });
                        }
                    })
                    .await;
                }
                let (count, status) = match client.extract_memories(&extract_text, &user_id, &agent_id).await {
                    Ok(entries) => (entries.len() as i64, "success".to_string()),
                    Err(e) => {
                        warn!(session_id = %session_id, "Post-compaction memory extraction failed: {}", e);
                        (0, "failure".to_string())
                    }
                };
                let pool = db_clone.0.clone();
                let log_id_cl = log_id.clone();
                let cloud_cl3 = cloud_cl.clone();
                let count_cl = count;
                let status_cl = status.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    if let Ok(conn) = pool.get() {
                        let _ = conn.execute(
                            "UPDATE memory_extraction_log SET memories_extracted = ?1, status = ?2 WHERE id = ?3",
                            rusqlite::params![count, status, log_id],
                        );
                    }
                })
                .await;
                if let Some(cl) = cloud_cl3 {
                    let _ = cl.patch_memory_extraction_log(&log_id_cl, serde_json::json!({
                        "memories_extracted": count_cl, "status": status_cl,
                    })).await;
                }
            });
        }
    }

    Ok(())
}
