use tracing::{debug, info, warn};
use ulid::Ulid;

use crate::db::connection::DbPool;
use crate::events::emitter::{emit_chat_context_update, emit_compaction_status};
use crate::executor::llm_provider::{
    model_context_window, ChatMessage, ContentBlock, LlmConfig, LlmProvider,
};
use crate::executor::memory::MemoryClient;
use crate::executor::workspace::AgentWorkspaceConfig;

const DEFAULT_COMPACTION_THRESHOLD: f64 = 0.65;
const DEFAULT_COMPACTION_RETAIN_COUNT: u32 = 12;
const COMPACTION_MAX_TOKENS: u32 = 4096;
const MIN_MESSAGES_TO_COMPACT: usize = 5;

/// Max consecutive auto-compaction failures before the circuit breaker opens.
const CIRCUIT_BREAKER_MAX_FAILURES: i64 = 3;

const COMPACTION_SYSTEM_PROMPT: &str = r#"You are a conversation summarizer. Your job is to produce a structured summary that will replace the original messages in context. Organize your summary using the following sections. Omit any section that has no relevant content.

## Goal
The user's primary objective or task in this conversation.

## Decisions Made
Key choices, trade-offs, or agreements reached.

## Key Technical Details
Code snippets, file paths, function names, configuration values, error messages, and other concrete details that were discussed.

## Completed Work
What has been accomplished so far.

## Pending / In Progress
Open questions, unfinished tasks, or next steps.

## User Preferences
Communication style, tooling preferences, or constraints the user expressed.

Be thorough but concise. The summary will replace these messages in the conversation context, so include everything needed to continue the conversation coherently."#;

#[derive(Debug, Clone)]
struct StoredChatMessage {
    id: String,
    created_at: String,
    message: ChatMessage,
}

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
    model_context_window(&config.model)
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

    // Minimum message guard — don't waste an LLM call for tiny sets
    if to_compact.len() < MIN_MESSAGES_TO_COMPACT {
        return None;
    }

    Some((to_compact, to_keep))
}

// ─── Circuit breaker helpers ────────────────────────────────────────────────

/// Returns true if the circuit breaker is open (too many recent auto-compaction failures).
pub fn is_circuit_open(db: &DbPool, session_id: &str) -> Result<bool, String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    let count: i64 = conn
        .query_row(
            "SELECT compaction_failure_count FROM chat_sessions WHERE id = ?1",
            rusqlite::params![session_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Ok(count >= CIRCUIT_BREAKER_MAX_FAILURES)
}

/// Increments the compaction failure counter for a session.
pub fn record_compaction_failure(db: &DbPool, session_id: &str) -> Result<(), String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE chat_sessions SET compaction_failure_count = compaction_failure_count + 1, compaction_last_failure_at = ?1, updated_at = ?1 WHERE id = ?2",
        rusqlite::params![now, session_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Resets the compaction failure counter (called inside the compaction transaction on success).
fn reset_compaction_failures_in_tx(
    tx: &rusqlite::Transaction,
    session_id: &str,
) -> Result<(), String> {
    tx.execute(
        "UPDATE chat_sessions SET compaction_failure_count = 0, compaction_last_failure_at = NULL WHERE id = ?1",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Estimate token count from text length (~4 chars per token).
fn estimate_tokens_from_text(text: &str) -> u32 {
    (text.len() as u32) / 4
}

fn summary_message_token_count(summary_content: &str, provider_output_tokens: u32) -> u32 {
    if provider_output_tokens > 0 {
        provider_output_tokens
    } else {
        estimate_tokens_from_text(summary_content)
    }
}

/// Extract plain text from a list of ChatMessages for token estimation.
fn extract_plain_text(messages: &[ChatMessage]) -> String {
    let mut text = String::new();
    for msg in messages {
        for block in &msg.content {
            match block {
                ContentBlock::Text { text: t } => {
                    text.push_str(t);
                    text.push('\n');
                }
                ContentBlock::Thinking { thinking } => {
                    text.push_str(thinking);
                    text.push('\n');
                }
                ContentBlock::ToolUse { name, input, .. } => {
                    text.push_str(name);
                    text.push_str(&input.to_string());
                    text.push('\n');
                }
                ContentBlock::ToolResult { content, .. } => {
                    text.push_str(content);
                    text.push('\n');
                }
                ContentBlock::Image { .. } => {}
            }
        }
    }
    text
}

fn summary_created_at(first_retained_created_at: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(first_retained_created_at)
        .map(|dt| (dt - chrono::Duration::milliseconds(1)).to_rfc3339())
        .unwrap_or_else(|_| first_retained_created_at.to_string())
}

async fn sync_compaction_to_cloud(
    client: std::sync::Arc<crate::db::cloud::SupabaseClient>,
    session_id: String,
    compacted_msg_ids: Vec<String>,
    summary_msg_id: String,
    summary_content_json: String,
    summary_created_at: String,
    summary_message_token_count: u32,
    compaction_id: String,
    compacted_ids_json: String,
    estimated_tokens: u32,
    updated_at: String,
) {
    if let Err(e) = client
        .upsert_chat_message_with_metadata(
            &summary_msg_id,
            &session_id,
            "assistant",
            &summary_content_json,
            Some(summary_message_token_count as i64),
            false,
            &summary_created_at,
        )
        .await
    {
        warn!(session_id = %session_id, "cloud upsert summary chat_message: {}", e);
    }

    for message_id in compacted_msg_ids {
        if let Err(e) = client
            .patch_by_id(
                "chat_messages",
                &message_id,
                serde_json::json!({ "is_compacted": true }),
            )
            .await
        {
            warn!(session_id = %session_id, message_id = %message_id, "cloud patch compacted chat_message: {}", e);
        }
    }

    if let Err(e) = client
        .upsert_chat_compaction_summary(
            &compaction_id,
            &session_id,
            &summary_msg_id,
            &compacted_ids_json,
            None,
            summary_message_token_count as i64,
            &updated_at,
        )
        .await
    {
        warn!(session_id = %session_id, "cloud upsert chat_compaction_summary: {}", e);
    }

    if let Err(e) = client
        .patch_by_id(
            "chat_sessions",
            &session_id,
            serde_json::json!({
                "last_input_tokens": estimated_tokens,
                "updated_at": updated_at,
            }),
        )
        .await
    {
        warn!(session_id = %session_id, "cloud patch chat_session after compaction: {}", e);
    }
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
    // Emit compaction started
    emit_compaction_status(app, session_id, "started");

    let result = perform_compaction_inner(
        agent_id,
        session_id,
        provider,
        ws_config,
        app,
        db,
        memory_client,
        memory_user_id,
        cloud_client,
    )
    .await;

    match &result {
        Ok(()) => emit_compaction_status(app, session_id, "completed"),
        Err(_) => emit_compaction_status(app, session_id, "failed"),
    }

    result
}

async fn perform_compaction_inner(
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
        tokio::task::spawn_blocking(move || -> Result<Vec<StoredChatMessage>, String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            let mut stmt = conn
                .prepare(
                    "SELECT id, role, content, created_at FROM chat_messages
                     WHERE session_id = ?1 AND is_compacted = 0
                     ORDER BY created_at ASC",
                )
                .map_err(|e| e.to_string())?;

            let msgs = stmt
                .query_map(rusqlite::params![sid], |row| {
                    let id: String = row.get(0)?;
                    let role: String = row.get(1)?;
                    let content_json: String = row.get(2)?;
                    let created_at: String = row.get(3)?;
                    Ok((id, role, content_json, created_at))
                })
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .map(|(id, role, content_json, created_at)| {
                    let content: Vec<ContentBlock> =
                        serde_json::from_str(&content_json).unwrap_or_default();
                    StoredChatMessage {
                        id,
                        created_at: created_at.clone(),
                        message: ChatMessage {
                            role,
                            content,
                            created_at: Some(created_at),
                        },
                    }
                })
                .collect();

            Ok(msgs)
        })
        .await
        .map_err(|e| e.to_string())??
    };

    let retain_count = effective_retain_count(ws_config);
    let msg_ids: Vec<String> = messages.iter().map(|msg| msg.id.clone()).collect();
    let chat_messages: Vec<ChatMessage> = messages.iter().map(|msg| msg.message.clone()).collect();

    // 2. Determine what to compact
    let (to_compact, to_keep) = match select_messages_for_compaction(&chat_messages, retain_count) {
        Some(split) => split,
        None => {
            debug!("Not enough messages to compact for session {}", session_id);
            return Ok(());
        }
    };

    let compact_count = to_compact.len();
    let compacted_msg_ids: Vec<String> = msg_ids[..compact_count].to_vec();
    let summary_created_at = messages
        .get(compact_count)
        .map(|msg| summary_created_at(&msg.created_at))
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

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
    let summary_message_token_count =
        summary_message_token_count(&summary_content, response.usage.output_tokens);

    // Bug 4 fix: compute retained-text estimate from deserialized messages, not JSON LENGTH()
    let retained_text = extract_plain_text(&to_keep);
    let retained_text_tokens = estimate_tokens_from_text(&retained_text);

    // 4. Persist in a single transaction: insert summary, flag compacted, record compaction, update session
    let pool = db.0.clone();
    let sid = sid.clone();
    let compacted_ids_json =
        serde_json::to_string(&compacted_msg_ids).map_err(|e| e.to_string())?;
    let summary_msg_id = Ulid::new().to_string();
    let compaction_id = Ulid::new().to_string();

    let estimated_tokens = retained_text_tokens + summary_message_token_count;
    let summary_content_json = serde_json::to_string(&vec![ContentBlock::Text {
        text: summary_content,
    }])
    .map_err(|e| e.to_string())?;
    let cloud_compacted_msg_ids = compacted_msg_ids.clone();
    let cloud_summary_msg_id = summary_msg_id.clone();
    let cloud_summary_content_json = summary_content_json.clone();
    let cloud_summary_created_at = summary_created_at.clone();
    let cloud_compaction_id = compaction_id.clone();
    let cloud_compacted_ids_json = compacted_ids_json.clone();
    let cloud_session_id = sid.clone();

    let est = estimated_tokens;
    let updated_at = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let mut conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();

        // Begin transaction — all writes succeed or none do
        let tx = conn.transaction().map_err(|e| e.to_string())?;

        tx.execute(
            "INSERT INTO chat_messages (id, session_id, role, content, created_at, token_count, is_compacted)
             VALUES (?1, ?2, 'assistant', ?3, ?4, ?5, 0)",
            rusqlite::params![
                summary_msg_id,
                sid,
                summary_content_json,
                summary_created_at,
                summary_message_token_count
            ],
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
        tx.execute(&sql, rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())))
            .map_err(|e| e.to_string())?;

        // Record compaction — store NULL for original_token_count (Bug 2)
        tx.execute(
            "INSERT INTO chat_compaction_summaries (id, session_id, summary_message_id, compacted_message_ids, original_token_count, summary_token_count, created_at)
             VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6)",
            rusqlite::params![
                compaction_id,
                sid,
                summary_msg_id,
                compacted_ids_json,
                summary_message_token_count,
                now
            ],
        )
        .map_err(|e| e.to_string())?;

        // Update session with estimated remaining tokens
        tx.execute(
            "UPDATE chat_sessions SET last_input_tokens = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![est, now, sid],
        )
        .map_err(|e| e.to_string())?;

        // Reset circuit breaker on success
        reset_compaction_failures_in_tx(&tx, &sid)?;

        tx.commit().map_err(|e| e.to_string())?;
        Ok(now)
    })
    .await
    .map_err(|e| e.to_string())??;

    if let Some(client) = cloud_client {
        tauri::async_runtime::spawn(sync_compaction_to_cloud(
            client,
            cloud_session_id,
            cloud_compacted_msg_ids,
            cloud_summary_msg_id,
            cloud_summary_content_json,
            cloud_summary_created_at,
            summary_message_token_count,
            cloud_compaction_id,
            cloud_compacted_ids_json,
            estimated_tokens,
            updated_at,
        ));
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
            tauri::async_runtime::spawn(async move {
                let log_id = Ulid::new().to_string();
                let now = chrono::Utc::now().to_rfc3339();
                {
                    let pool = db_clone.0.clone();
                    let log_id = log_id.clone();
                    let sid = session_id.clone();
                    let aid = agent_id.clone();
                    let now = now.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        if let Ok(conn) = pool.get() {
                            let _ = conn.execute(
                                "INSERT INTO memory_extraction_log (id, session_id, agent_id, memories_extracted, status, created_at)
                                 VALUES (?1, ?2, ?3, 0, 'running', ?4)",
                                rusqlite::params![log_id, sid, aid, now],
                            );
                        }
                    })
                    .await;
                }
                let (count, status) = match client.extract_memories(&extract_text, &user_id).await {
                    Ok(entries) => (entries.len() as i64, "success".to_string()),
                    Err(e) => {
                        warn!(session_id = %session_id, "Post-compaction memory extraction failed: {}", e);
                        (0, "failure".to_string())
                    }
                };
                let pool = db_clone.0.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    if let Ok(conn) = pool.get() {
                        let _ = conn.execute(
                            "UPDATE memory_extraction_log SET memories_extracted = ?1, status = ?2 WHERE id = ?3",
                            rusqlite::params![count, status, log_id],
                        );
                    }
                })
                .await;
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{select_messages_for_compaction, summary_created_at, summary_message_token_count};
    use crate::executor::llm_provider::{ChatMessage, ContentBlock};

    fn text_message(text: &str) -> ChatMessage {
        ChatMessage {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            created_at: None,
        }
    }

    #[test]
    fn compaction_guard_skips_tiny_prefixes() {
        let messages = vec![
            text_message("1"),
            text_message("2"),
            text_message("3"),
            text_message("4"),
            text_message("5"),
            text_message("6"),
            text_message("7"),
            text_message("8"),
            text_message("9"),
            text_message("10"),
            text_message("11"),
            text_message("12"),
            text_message("13"),
            text_message("14"),
            text_message("15"),
            text_message("16"),
        ];

        assert!(select_messages_for_compaction(&messages, 12).is_none());
        assert!(select_messages_for_compaction(&messages, 10).is_some());
    }

    #[test]
    fn summary_timestamp_sorts_before_first_retained_message() {
        let retained = "2026-04-04T12:00:00Z";
        let summary = summary_created_at(retained);

        assert!(summary.as_str() < retained);
    }

    #[test]
    fn summary_token_count_falls_back_when_provider_reports_zero() {
        let summary = "[Conversation Summary]\nhello world";

        assert_eq!(
            summary_message_token_count(summary, 0),
            (summary.len() as u32) / 4
        );
        assert_eq!(summary_message_token_count(summary, 17), 17);
    }
}
