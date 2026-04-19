use std::sync::Arc;

use tauri::Manager;
use tracing::warn;
use ulid::Ulid;

use crate::db::DbPool;
use crate::events::emitter::{
    emit_agent_iteration, emit_agent_tool_result, emit_chat_context_update,
};
use crate::executor::agent_tools::ToolExecutionContext;
use crate::executor::compaction;
use crate::executor::context::{self, ContextMode, ContextRequest};
use crate::executor::engine::{
    AgentSemaphores, RunRequest, SessionExecutionRegistry, UserQuestionRegistry,
};
use crate::executor::keychain;
use crate::executor::llm_provider::{
    self, AgentMcpWiring, ChatMessage, ContentBlock, LlmConfig, LlmProvider, ToolDefinition,
};
use crate::executor::mcp_server::McpServerHandle;
use crate::executor::memory::MemoryClient;
use crate::executor::permissions::{self, PermissionRegistry};
use crate::executor::session_worktree;
use crate::executor::workspace;

const DEFAULT_MAX_ITERATIONS: u32 = 25;
const DEFAULT_MAX_TOKENS_PER_CALL: u32 = 16384;
const LLM_RETRY_ATTEMPTS: u32 = 3;
const LLM_RETRY_BASE_DELAY_MS: u64 = 2000;
const MAX_AUTO_CONTINUATIONS: u32 = 2;
const AUTO_CONTINUE_REMINDER: &str = "You are not done yet. Continue working until the current task is complete. Do not stop after partial progress. If you are blocked, state the blocker explicitly and ask only for the missing input or permission.";

pub async fn run_agent_session(
    agent_id: &str,
    session_id: &str,
    chain_depth: i64,
    is_sub_agent: bool,
    allow_sub_agents: bool,
    db: &DbPool,
    app: &tauri::AppHandle,
    executor_tx: &tokio::sync::mpsc::UnboundedSender<RunRequest>,
    agent_semaphores: &AgentSemaphores,
    session_registry: &SessionExecutionRegistry,
    permission_registry: &PermissionRegistry,
    user_question_registry: Option<&UserQuestionRegistry>,
    memory_client: Option<&MemoryClient>,
    memory_user_id: &str,
    cloud_client: Option<std::sync::Arc<crate::db::cloud::SupabaseClient>>,
) -> Result<String, String> {
    let stream_id = format!("chat:{}", session_id);
    let ws_config = workspace::load_agent_config(agent_id).unwrap_or_default();
    let max_iterations = if ws_config.max_iterations > 0 {
        ws_config.max_iterations
    } else {
        DEFAULT_MAX_ITERATIONS
    };
    let max_total_tokens = u32::MAX;

    let semaphore = agent_semaphores.get_or_create(agent_id, db).await;
    let _permit = semaphore.acquire_owned().await.map_err(|e| e.to_string())?;

    session_registry.clear_cancelled(session_id).await;

    if is_session_cancelled(session_id, db, session_registry).await {
        return Err(finalize_cancelled_session(db, session_id).await);
    }

    update_session_execution_state(db, session_id, "running", None, None).await?;

    let provider_name = &ws_config.provider;
    let api_key = keychain::retrieve_api_key(provider_name).map_err(|_| {
        format!(
            "No API key configured for provider '{}'. Set it in Settings.",
            provider_name
        )
    })?;

    let history = load_session_messages(db, session_id).await?;
    let worktree_state = session_worktree::load_session_worktree_state(db, session_id).await?;
    let project_id = session_worktree::load_session_project_id(db, session_id).await?;
    if let Some(pid) = project_id.as_deref() {
        crate::commands::projects::assert_agent_in_project(db, pid, agent_id).await?;
        if let Err(e) = workspace::init_project_workspace(pid) {
            warn!(project_id = pid, "failed to init project workspace: {}", e);
        }
    }
    let session_type = load_session_type(db, session_id).await.ok();
    let pipeline = context::default_pipeline(memory_client.cloned());
    let allowed_tools = ContextRequest::effective_allowed_tools(&ws_config);
    let ctx_request = ContextRequest {
        agent_id: agent_id.to_string(),
        mode: ContextMode::Chat,
        session_id: Some(session_id.to_string()),
        session_type,
        project_id: project_id.clone(),
        goal: None,
        ws_config: ws_config.clone(),
        allowed_tools,
        existing_messages: Some(history),
        is_sub_agent,
        allow_sub_agents,
        chain_depth,
        user_id: memory_user_id.to_string(),
    };
    let snapshot = pipeline.build(&ctx_request, db).await?;
    let tools = snapshot.tools;
    let context_window = snapshot.token_budget.context_window;

    let llm_config = LlmConfig {
        model: ws_config.model.clone(),
        max_tokens: DEFAULT_MAX_TOKENS_PER_CALL,
        temperature: Some(ws_config.temperature),
        system_prompt: snapshot.system_prompt,
    };

    let tool_ctx = if is_sub_agent {
        ToolExecutionContext::new_for_sub_agent(
            agent_id,
            &stream_id,
            Some(session_id),
            chain_depth,
            db.clone(),
            executor_tx.clone(),
            app.clone(),
            agent_semaphores.clone(),
            session_registry.clone(),
            worktree_state.clone(),
            project_id.as_deref(),
        )
        .with_permission_registry(permission_registry.clone())
        .with_allow_sub_agents(allow_sub_agents)
        .with_memory_client(memory_client.cloned())
        .with_memory_user_id(memory_user_id.to_string())
        .with_cloud_client(cloud_client.clone())
    } else {
        ToolExecutionContext::new_with_bus(
            agent_id,
            &stream_id,
            Some(session_id),
            chain_depth,
            db.clone(),
            executor_tx.clone(),
            app.clone(),
            agent_semaphores.clone(),
            session_registry.clone(),
            worktree_state,
            project_id.as_deref(),
        )
        .with_permission_registry(permission_registry.clone())
        .with_memory_client(memory_client.cloned())
        .with_memory_user_id(memory_user_id.to_string())
        .with_cloud_client(cloud_client.clone())
    };
    let tool_ctx = if let Some(question_registry) = user_question_registry {
        tool_ctx.with_user_question_registry(question_registry.clone())
    } else {
        tool_ctx
    };
    let tool_ctx = Arc::new(tool_ctx);

    // ── Create provider (wiring MCP bridge for CLI providers) ────────────
    let mcp_handle: Option<McpServerHandle> = app
        .try_state::<McpServerHandle>()
        .map(|s| s.inner().clone());
    let wiring = mcp_handle.map(|handle| AgentMcpWiring {
        handle,
        agent_id: agent_id.to_string(),
        run_id: stream_id.clone(),
        tool_ctx: tool_ctx.clone(),
        tools: tools.clone(),
        permission_registry: permission_registry.clone(),
        app: app.clone(),
        db: db.clone(),
    });
    let provider = llm_provider::create_provider_with_mcp(provider_name, api_key, wiring)?;

    let result = run_session_loop(
        &provider,
        &llm_config,
        snapshot.messages,
        &tools,
        tool_ctx.as_ref(),
        &stream_id,
        session_id,
        max_iterations,
        max_total_tokens,
        context_window,
        &ws_config,
        app,
        db,
        session_registry,
        permission_registry,
    )
    .await;

    match result {
        Ok(summary) => Ok(summary),
        Err(reason) if reason == "cancelled" => {
            Err(finalize_cancelled_session(db, session_id).await)
        }
        Err(reason) => {
            finalize_failed_session(db, session_id, &reason).await?;
            Err(reason)
        }
    }
}

pub async fn run_session_loop(
    provider: &Box<dyn LlmProvider>,
    llm_config: &LlmConfig,
    mut messages: Vec<ChatMessage>,
    tools: &[ToolDefinition],
    tool_ctx: &ToolExecutionContext,
    stream_id: &str,
    session_id: &str,
    max_iterations: u32,
    max_total_tokens: u32,
    context_window: u32,
    ws_config: &workspace::AgentWorkspaceConfig,
    app: &tauri::AppHandle,
    db: &DbPool,
    session_registry: &SessionExecutionRegistry,
    permission_registry: &PermissionRegistry,
) -> Result<String, String> {
    let mut cumulative_input_tokens: u32 = 0;
    let mut cumulative_output_tokens: u32 = 0;
    let mut iteration: u32 = 0;
    let mut finish_summary: Option<String> = None;
    let mut auto_continue_count: u32 = 0;

    loop {
        if is_session_cancelled(session_id, db, session_registry).await {
            return Err("cancelled".to_string());
        }

        if iteration >= max_iterations {
            messages.push(ChatMessage {
        role: "user".to_string(),
        content: vec![ContentBlock::Text {
          text: "You have reached the maximum number of iterations. Please provide a final summary of what you accomplished and what remains to be done.".to_string(),
        }],
        created_at: None,
      });
            let response = call_llm_with_retry(
                provider.as_ref(),
                llm_config,
                &messages,
                &[],
                app,
                stream_id,
                iteration,
            )
            .await?;
            cumulative_input_tokens += response.usage.input_tokens;
            cumulative_output_tokens += response.usage.output_tokens;
            if let Some(summary) = extract_text_summary(&response.content) {
                finish_summary = Some(summary);
            }
            save_chat_message(
                &db.0,
                session_id,
                "assistant",
                &response.content,
                tool_ctx.cloud_client.clone(),
            )
            .await?;
            break;
        }

        if cumulative_input_tokens + cumulative_output_tokens >= max_total_tokens {
            finish_summary = Some(format!(
                "Stopped after exceeding token budget ({} tokens).",
                max_total_tokens
            ));
            break;
        }

        iteration += 1;
        emit_agent_iteration(
            app,
            stream_id,
            iteration,
            "llm_call",
            None,
            cumulative_input_tokens + cumulative_output_tokens,
        );

        let response = call_llm_with_retry(
            provider.as_ref(),
            llm_config,
            &messages,
            tools,
            app,
            stream_id,
            iteration,
        )
        .await?;
        cumulative_input_tokens += response.usage.input_tokens;
        cumulative_output_tokens += response.usage.output_tokens;

        if is_session_cancelled(session_id, db, session_registry).await {
            return Err("cancelled".to_string());
        }

        save_chat_message(
            &db.0,
            session_id,
            "assistant",
            &response.content,
            tool_ctx.cloud_client.clone(),
        )
        .await?;

        match response.stop_reason {
            llm_provider::StopReason::EndTurn => {
                if finish_summary.is_none() {
                    finish_summary = extract_text_summary(&response.content);
                }
                let should_auto_continue = auto_continue_count < MAX_AUTO_CONTINUATIONS
                    && should_auto_continue_after_end_turn(&response.content);
                messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: response.content,
                    created_at: None,
                });
                if should_auto_continue {
                    auto_continue_count += 1;
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: vec![ContentBlock::Text {
                            text: AUTO_CONTINUE_REMINDER.to_string(),
                        }],
                        created_at: None,
                    });
                    finish_summary = None;
                    continue;
                }
                break;
            }

            llm_provider::StopReason::ToolUse => {
                messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: response.content.clone(),
                    created_at: None,
                });

                let mut tool_results: Vec<ContentBlock> = Vec::new();
                let mut should_finish = false;

                for block in &response.content {
                    if let ContentBlock::ToolUse { id, name, input } = block {
                        emit_agent_iteration(
                            app,
                            stream_id,
                            iteration,
                            "tool_exec",
                            Some(name),
                            cumulative_input_tokens + cumulative_output_tokens,
                        );

                        let perm_reg = tool_ctx
                            .permission_registry
                            .as_ref()
                            .unwrap_or(permission_registry);
                        match permissions::execute_tool_with_permissions(
                            tool_ctx, name, input, app, stream_id, perm_reg,
                        )
                        .await
                        {
                            Ok((result, is_finish)) => {
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
                                if is_finish {
                                    finish_summary = Some(result);
                                    should_finish = true;
                                }
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
                    }
                }

                if !tool_results.is_empty() {
                    save_chat_message(
                        &db.0,
                        session_id,
                        "user",
                        &tool_results,
                        tool_ctx.cloud_client.clone(),
                    )
                    .await?;
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: tool_results,
                        created_at: None,
                    });
                }

                if should_finish {
                    break;
                }
            }

            llm_provider::StopReason::MaxTokens => {
                let mut tool_error_results: Vec<ContentBlock> = Vec::new();
                for block in &response.content {
                    if let ContentBlock::ToolUse { id, .. } = block {
                        tool_error_results.push(ContentBlock::ToolResult {
              tool_use_id: id.clone(),
              content: "Error: your previous tool call was truncated because the response exceeded the token limit. Please retry with a shorter input, or break the work into smaller steps.".to_string(),
              is_error: true,
            });
                    }
                }

                if !tool_error_results.is_empty() {
                    save_chat_message(
                        &db.0,
                        session_id,
                        "user",
                        &tool_error_results,
                        tool_ctx.cloud_client.clone(),
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
            text: "Your response was cut off due to the token limit. Please continue where you left off. If you were writing a file, try breaking the content into smaller pieces.".to_string(),
          }],
          created_at: None,
        });
            }
        }
    }

    let total_tokens = cumulative_input_tokens + cumulative_output_tokens;
    emit_agent_iteration(app, stream_id, iteration, "finished", None, total_tokens);
    emit_chat_context_update(
        app,
        session_id,
        cumulative_input_tokens,
        cumulative_output_tokens,
        context_window,
    );
    update_session_execution_state(db, session_id, "success", finish_summary.clone(), None).await?;
    update_last_input_tokens(db, session_id, cumulative_input_tokens).await;

    // Post-session memory extraction
    if ws_config.memory_enabled {
        if let Some(client) = tool_ctx.memory_client.clone() {
            let agent_id = tool_ctx.agent_id.clone();
            let user_id = tool_ctx.memory_user_id.clone();
            let conversation_text = build_conversation_text(&messages);
            let db_clone = DbPool(db.0.clone());
            let session_id_str = session_id.to_string();
            tauri::async_runtime::spawn(async move {
                extract_session_memories(
                    client,
                    conversation_text,
                    &user_id,
                    &agent_id,
                    &session_id_str,
                    &db_clone,
                )
                .await;
            });
        }
    }

    let threshold = compaction::effective_threshold(ws_config);
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
            let agent_id = tool_ctx.agent_id.clone();
            let session_id = session_id.to_string();
            let ws_config = ws_config.clone();
            let app = app.clone();
            let db = DbPool(db.0.clone());
            let mem_client = tool_ctx.memory_client.clone();
            let compaction_user_id = tool_ctx.memory_user_id.clone();
            let compact_cloud_client = tool_ctx.cloud_client.clone();
            match keychain::retrieve_api_key(&ws_config.provider) {
                Ok(api_key) => match llm_provider::create_provider(&ws_config.provider, api_key) {
                    Ok(provider) => {
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = compaction::perform_compaction(
                                &agent_id,
                                &session_id,
                                provider.as_ref(),
                                &ws_config,
                                &app,
                                &db,
                                mem_client,
                                &compaction_user_id,
                                compact_cloud_client,
                            )
                            .await
                            {
                                warn!(session_id = %session_id, "Session compaction failed: {}", e);
                                let _ = compaction::record_compaction_failure(&db, &session_id);
                            }
                        });
                    }
                    Err(e) => {
                        warn!(session_id = %session_id, "Session compaction provider setup failed: {}", e);
                        let _ = compaction::record_compaction_failure(&db, &session_id);
                    }
                },
                Err(_) => {
                    warn!(session_id = %session_id, "Session compaction skipped: no API key for provider '{}'", ws_config.provider);
                    let _ = compaction::record_compaction_failure(&db, &session_id);
                }
            }
        }
    }

    Ok(finish_summary.unwrap_or_else(|| "Agent session completed.".to_string()))
}

pub async fn save_chat_message(
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
        let msg_id = ulid::Ulid::new().to_string();
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

pub async fn update_session_execution_state(
    db: &DbPool,
    session_id: &str,
    execution_state: &str,
    finish_summary: Option<String>,
    terminal_error: Option<String>,
) -> Result<(), String> {
    let pool = db.0.clone();
    let session_id = session_id.to_string();
    let execution_state = execution_state.to_string();
    let reset_metadata = execution_state == "running";
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE chat_sessions
       SET execution_state = ?1,
           finish_summary = CASE
             WHEN ?6 THEN NULL
             WHEN ?2 IS NULL THEN finish_summary
             ELSE ?2
           END,
           terminal_error = CASE
             WHEN ?6 THEN NULL
             WHEN ?3 IS NULL THEN terminal_error
             ELSE ?3
           END,
           updated_at = ?4
       WHERE id = ?5 AND COALESCE(execution_state, '') != 'cancelled'",
            rusqlite::params![
                execution_state,
                finish_summary,
                terminal_error,
                now,
                session_id,
                reset_metadata,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;

    Ok(())
}

pub async fn finalize_failed_session(
    db: &DbPool,
    session_id: &str,
    reason: &str,
) -> Result<(), String> {
    update_session_execution_state(
        db,
        session_id,
        if reason == "timed out" {
            "timed_out"
        } else {
            "failure"
        },
        None,
        Some(reason.to_string()),
    )
    .await
}

pub async fn finalize_cancelled_session(db: &DbPool, session_id: &str) -> String {
    let _ = update_session_execution_state(
        db,
        session_id,
        "cancelled",
        None,
        Some("Cancelled".to_string()),
    )
    .await;
    "cancelled".to_string()
}

async fn update_last_input_tokens(db: &DbPool, session_id: &str, input_tokens: u32) {
    let pool = db.0.clone();
    let session_id = session_id.to_string();
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(conn) = pool.get() {
            let now = chrono::Utc::now().to_rfc3339();
            let _ = conn.execute(
                "UPDATE chat_sessions SET last_input_tokens = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![input_tokens, now, session_id],
            );
        }
    })
    .await;
}

async fn load_session_messages(db: &DbPool, session_id: &str) -> Result<Vec<ChatMessage>, String> {
    let pool = db.0.clone();
    let session_id = session_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<Vec<ChatMessage>, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT role, content
       FROM chat_messages
       WHERE session_id = ?1 AND is_compacted = 0
       ORDER BY created_at ASC",
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
                let content: Vec<ContentBlock> =
                    serde_json::from_str(&content_json).unwrap_or_default();
                ChatMessage {
                    role,
                    content,
                    created_at: None,
                }
            })
            .collect();

        Ok(messages)
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn is_session_cancelled(
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

fn build_conversation_text(messages: &[ChatMessage]) -> String {
    let mut text = String::new();
    for msg in messages {
        let label = if msg.role == "user" {
            "User"
        } else {
            "Assistant"
        };
        for block in &msg.content {
            match block {
                ContentBlock::Text { text: t } => {
                    text.push_str(&format!("{}: {}\n\n", label, t));
                }
                ContentBlock::ToolUse { name, input, .. } => {
                    text.push_str(&format!(
                        "{} used tool `{}` with input: {}\n\n",
                        label, name, input
                    ));
                }
                ContentBlock::ToolResult { content, .. } => {
                    text.push_str(&format!("Tool result: {}\n\n", content));
                }
                _ => {}
            }
        }
    }
    text
}

async fn extract_session_memories(
    client: MemoryClient,
    conversation_text: String,
    user_id: &str,
    agent_id: &str,
    session_id: &str,
    db: &DbPool,
) {
    if conversation_text.trim().is_empty() {
        return;
    }

    let log_id = Ulid::new().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    {
        let pool = db.0.clone();
        let log_id = log_id.clone();
        let sid = session_id.to_string();
        let aid = agent_id.to_string();
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

    let (count, status) = match client.extract_memories(&conversation_text, user_id).await {
        Ok(entries) => (entries.len() as i64, "success".to_string()),
        Err(e) => {
            warn!(
                session_id = session_id,
                "Post-session memory extraction failed: {}", e
            );
            (0, "failure".to_string())
        }
    };

    let pool = db.0.clone();
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(conn) = pool.get() {
            let _ = conn.execute(
        "UPDATE memory_extraction_log SET memories_extracted = ?1, status = ?2 WHERE id = ?3",
        rusqlite::params![count, status, log_id],
      );
        }
    })
    .await;
}

fn extract_text_summary(content: &[ContentBlock]) -> Option<String> {
    for block in content.iter().rev() {
        if let ContentBlock::Text { text } = block {
            if !text.trim().is_empty() {
                return Some(text.clone());
            }
        }
    }
    None
}

pub(crate) fn should_auto_continue_after_end_turn(content: &[ContentBlock]) -> bool {
    let Some(summary) = extract_text_summary(content) else {
        return false;
    };

    let lower = summary.to_lowercase();
    [
        "almost done",
        "still need",
        "still have to",
        "let me finish",
        "let me continue",
        "i'll continue",
        "i will continue",
        "remaining work",
        "remaining step",
        "one more thing to do",
        "not finished yet",
        "not done yet",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

async fn call_llm_with_retry(
    provider: &dyn LlmProvider,
    config: &LlmConfig,
    messages: &[ChatMessage],
    tools: &[ToolDefinition],
    app: &tauri::AppHandle,
    stream_id: &str,
    iteration: u32,
) -> Result<llm_provider::LlmResponse, String> {
    let mut last_error = String::new();

    for attempt in 0..LLM_RETRY_ATTEMPTS {
        match provider
            .chat_streaming(config, messages, tools, app, stream_id, iteration)
            .await
        {
            Ok(response) => return Ok(response),
            Err(e) => {
                last_error = e.clone();
                if attempt < LLM_RETRY_ATTEMPTS - 1 {
                    let delay = LLM_RETRY_BASE_DELAY_MS * (1 << attempt);
                    warn!(
                      stream_id = stream_id,
                      attempt = attempt + 1,
                      error = %e,
                      delay_ms = delay,
                      "Session LLM call failed, retrying"
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                }
            }
        }
    }

    Err(format!(
        "LLM call failed after {} attempts: {}",
        LLM_RETRY_ATTEMPTS, last_error
    ))
}

async fn load_session_type(db: &DbPool, session_id: &str) -> Result<String, String> {
    let pool = db.0.clone();
    let sid = session_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<String, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT session_type FROM chat_sessions WHERE id = ?1",
            rusqlite::params![sid],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::should_auto_continue_after_end_turn;
    use crate::executor::llm_provider::ContentBlock;

    #[test]
    fn detects_incomplete_end_turn_language() {
        let content = vec![ContentBlock::Text {
            text: "Almost done! I updated two files but still need to create the main component. Let me finish.".to_string(),
        }];

        assert!(should_auto_continue_after_end_turn(&content));
    }

    #[test]
    fn ignores_normal_completed_response() {
        let content = vec![ContentBlock::Text {
            text: "Implemented the landing page, wired the styles, and verified the build passes."
                .to_string(),
        }];

        assert!(!should_auto_continue_after_end_turn(&content));
    }
}
