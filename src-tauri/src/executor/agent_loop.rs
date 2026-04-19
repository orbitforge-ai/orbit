use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tokio::sync::oneshot;
use tracing::{error, info, warn};

use crate::db::DbPool;
use crate::events::emitter::{emit_agent_iteration, emit_agent_tool_result, emit_log_chunk};
use crate::executor::agent_tools::ToolExecutionContext;
use crate::executor::context::{self, ContextMode, ContextRequest};
use crate::executor::engine::{AgentSemaphores, SessionExecutionRegistry};
use crate::executor::keychain;
use crate::executor::llm_provider::{
    self, AgentMcpWiring, ChatMessage, ContentBlock, LlmConfig, LlmProvider, ToolDefinition,
};
use crate::executor::mcp_server::McpServerHandle;
use crate::executor::memory::MemoryClient;
use crate::executor::permissions::{self, PermissionRegistry};
use crate::executor::process::ProcessResult;
use crate::executor::session_agent;
use crate::executor::session_worktree;
use crate::executor::workspace;
use crate::models::task::{AgentLoopConfig, AgentStepConfig};

const DEFAULT_MAX_ITERATIONS: u32 = 25;
const DEFAULT_MAX_TOKENS_PER_CALL: u32 = 16384;
const LLM_RETRY_ATTEMPTS: u32 = 3;
const LLM_RETRY_BASE_DELAY_MS: u64 = 2000;
const MAX_AUTO_CONTINUATIONS: u32 = 2;
const AUTO_CONTINUE_REMINDER: &str = "You are not done yet. Continue working until the current task is complete. Do not stop after partial progress. If you are blocked, state the blocker explicitly and ask only for the missing input or permission.";

// ─── Log writer ─────────────────────────────────────────────────────────────

/// Accumulates log lines AND emits them as Tauri events.
/// At the end, call `flush_to_file` to persist to disk.
struct AgentLog {
    buffer: Mutex<Vec<String>>,
}

impl AgentLog {
    fn new() -> Self {
        Self {
            buffer: Mutex::new(Vec::new()),
        }
    }

    /// Emit a log chunk to the frontend and buffer it for the log file.
    fn log(&self, app: &tauri::AppHandle, run_id: &str, lines: Vec<(String, String)>) {
        {
            let mut buf = self.buffer.lock().unwrap();
            for (stream, line) in &lines {
                if stream == "stderr" {
                    buf.push(format!("[stderr] {}", line));
                } else {
                    buf.push(line.clone());
                }
            }
        }
        emit_log_chunk(app, run_id, lines);
    }

    /// Write accumulated log content to the log file on disk.
    fn flush_to_file(&self, log_path: &PathBuf) {
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let buf = self.buffer.lock().unwrap();
        let content = buf.join("\n");
        if let Err(e) = std::fs::write(log_path, content) {
            warn!("failed to write agent log file: {}", e);
        }
    }
}

pub(crate) struct AgentLoopOutcome {
    pub finish_summary: Option<String>,
    pub iterations: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub duration_ms: i64,
}

// ─── Agent loop ─────────────────────────────────────────────────────────────

async fn execute_agent_loop_internal(
    run_id: &str,
    agent_id: &str,
    cfg: &AgentLoopConfig,
    log_path: &PathBuf,
    app: &tauri::AppHandle,
    mut cancel: Option<&mut oneshot::Receiver<()>>,
    db: &DbPool,
    executor_tx: &tokio::sync::mpsc::UnboundedSender<crate::executor::engine::RunRequest>,
    chain_depth: i64,
    is_sub_agent: bool,
    project_id: Option<&str>,
    persist_run_metadata: bool,
    agent_semaphores: &AgentSemaphores,
    session_registry: &SessionExecutionRegistry,
    permission_registry: &PermissionRegistry,
    memory_client: Option<&MemoryClient>,
    memory_user_id: &str,
    cloud_client: Option<std::sync::Arc<crate::db::cloud::SupabaseClient>>,
) -> Result<AgentLoopOutcome, String> {
    let start = std::time::Instant::now();
    let log = AgentLog::new();

    // ── Load workspace config ────────────────────────────────────────────
    let ws_config = workspace::load_agent_config(agent_id).unwrap_or_default();
    let model = cfg.model.clone().unwrap_or_else(|| ws_config.model.clone());
    let max_iterations = cfg
        .max_iterations
        .unwrap_or(if ws_config.max_iterations > 0 {
            ws_config.max_iterations
        } else {
            DEFAULT_MAX_ITERATIONS
        });
    let max_total_tokens = cfg.max_total_tokens.unwrap_or(u32::MAX);

    // ── Resolve provider + API key ───────────────────────────────────────
    let provider_name = &ws_config.provider;
    let api_key = keychain::retrieve_api_key(provider_name).map_err(|_| {
        let msg = format!(
            "No API key configured for provider '{}'. Set it in Settings.",
            provider_name
        );
        log.log(app, run_id, vec![("stderr".to_string(), msg.clone())]);
        log.flush_to_file(log_path);
        msg
    })?;

    // ── Apply template variable substitution to goal ─────────────────────
    let mut goal = cfg.goal.clone();
    if let Some(ref vars) = cfg.template_vars {
        for (key, value) in vars {
            goal = goal.replace(&format!("{{{{{}}}}}", key), value);
        }
    }

    if let Some(pid) = project_id {
        crate::commands::projects::assert_agent_in_project(db, pid, agent_id).await?;
        if let Err(e) = workspace::init_project_workspace(pid) {
            warn!(project_id = pid, "failed to init project workspace: {}", e);
        }
    }

    // ── Build context via pipeline ──────────────────────────────────────
    let pipeline = context::default_pipeline(memory_client.cloned());
    let allowed_tools = ContextRequest::effective_allowed_tools(&ws_config);
    let ctx_request = ContextRequest {
        agent_id: agent_id.to_string(),
        mode: ContextMode::AgentLoop,
        session_id: None,
        session_type: None,
        project_id: project_id.map(str::to_string),
        goal: Some(goal.clone()),
        ws_config: ws_config.clone(),
        allowed_tools,
        existing_messages: None,
        is_sub_agent,
        allow_sub_agents: !is_sub_agent,
        chain_depth,
        user_id: memory_user_id.to_string(),
    };
    let snapshot = pipeline.build(&ctx_request, db).await.map_err(|e| {
        log.log(app, run_id, vec![("stderr".to_string(), e.clone())]);
        log.flush_to_file(log_path);
        e
    })?;

    let llm_config = LlmConfig {
        model,
        max_tokens: DEFAULT_MAX_TOKENS_PER_CALL,
        temperature: Some(ws_config.temperature),
        system_prompt: snapshot.system_prompt,
    };

    let tools: Vec<ToolDefinition> = snapshot.tools;
    let tool_ctx = if is_sub_agent {
        ToolExecutionContext::new_for_sub_agent(
            agent_id,
            run_id,
            None,
            chain_depth,
            db.clone(),
            executor_tx.clone(),
            app.clone(),
            agent_semaphores.clone(),
            session_registry.clone(),
            None,
            project_id,
        )
        .with_permission_registry(permission_registry.clone())
        .with_allow_sub_agents(false)
        .with_memory_client(memory_client.cloned())
        .with_memory_user_id(memory_user_id.to_string())
        .with_cloud_client(cloud_client.clone())
    } else {
        ToolExecutionContext::new_with_bus(
            agent_id,
            run_id,
            None,
            chain_depth,
            db.clone(),
            executor_tx.clone(),
            app.clone(),
            agent_semaphores.clone(),
            session_registry.clone(),
            None,
            project_id,
        )
        .with_permission_registry(permission_registry.clone())
        .with_memory_client(memory_client.cloned())
        .with_memory_user_id(memory_user_id.to_string())
        .with_cloud_client(cloud_client.clone())
    };
    let tool_ctx = Arc::new(tool_ctx);

    // ── Create provider (wiring MCP bridge for CLI providers) ────────────
    let mcp_handle: Option<McpServerHandle> = app
        .try_state::<McpServerHandle>()
        .map(|s| s.inner().clone());
    let wiring = mcp_handle.map(|handle| AgentMcpWiring {
        handle,
        agent_id: agent_id.to_string(),
        run_id: run_id.to_string(),
        tool_ctx: tool_ctx.clone(),
        tools: tools.clone(),
        permission_registry: permission_registry.clone(),
        app: app.clone(),
        db: db.clone(),
    });
    let provider = llm_provider::create_provider_with_mcp(provider_name, api_key, wiring)
        .map_err(|e| {
            log.log(app, run_id, vec![("stderr".to_string(), e.clone())]);
            log.flush_to_file(log_path);
            e
        })?;

    // ── Init conversation ────────────────────────────────────────────────
    let mut messages: Vec<ChatMessage> = vec![ChatMessage {
        role: "user".to_string(),
        content: vec![ContentBlock::Text { text: goal.clone() }],
        created_at: None,
    }];

    log.log(
        app,
        run_id,
        vec![
            ("stdout".to_string(), "=== Agent Loop Start ===".to_string()),
            ("stdout".to_string(), format!("Goal: {}", goal)),
            (
                "stdout".to_string(),
                format!(
                    "Model: {} | Max iterations: {} | Token budget: {}",
                    llm_config.model, max_iterations, max_total_tokens
                ),
            ),
            ("stdout".to_string(), "".to_string()),
        ],
    );

    let mut cumulative_input_tokens: u32 = 0;
    let mut cumulative_output_tokens: u32 = 0;
    let mut iteration: u32 = 0;
    let mut finish_summary: Option<String> = None;
    let mut auto_continue_count: u32 = 0;

    // ── Main loop ────────────────────────────────────────────────────────
    loop {
        if let Some(cancel_rx) = cancel.as_mut() {
            match cancel_rx.try_recv() {
                Ok(()) => {
                    info!(run_id = run_id, "agent loop cancelled");
                    log.flush_to_file(log_path);
                    return Err("cancelled".to_string());
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    info!(run_id = run_id, "agent loop cancel channel closed");
                    log.flush_to_file(log_path);
                    return Err("cancelled".to_string());
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
            }
        }

        if iteration >= max_iterations {
            warn!(run_id = run_id, "agent loop hit iteration limit");
            log.log(
                app,
                run_id,
                vec![(
                    "stderr".to_string(),
                    format!(
                        "[iteration limit reached: {}/{}]",
                        iteration, max_iterations
                    ),
                )],
            );
            messages.push(ChatMessage {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: "You have reached the maximum number of iterations. Please provide a final summary of what you accomplished and what remains to be done.".to_string(),
                }],
                created_at: None,
            });
            if let Ok(resp) = call_llm_with_retry(
                provider.as_ref(),
                &llm_config,
                &messages,
                &[],
                app,
                run_id,
                iteration,
                &log,
            )
            .await
            {
                cumulative_input_tokens += resp.usage.input_tokens;
                cumulative_output_tokens += resp.usage.output_tokens;
                if finish_summary.is_none() {
                    finish_summary = extract_text_summary(&resp.content);
                }
                messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: resp.content,
                    created_at: None,
                });
            }
            break;
        }

        let total_tokens = cumulative_input_tokens + cumulative_output_tokens;
        if total_tokens >= max_total_tokens {
            warn!(run_id = run_id, "agent loop hit token budget");
            log.log(
                app,
                run_id,
                vec![(
                    "stderr".to_string(),
                    format!(
                        "[token budget exceeded: {}/{}]",
                        total_tokens, max_total_tokens
                    ),
                )],
            );
            break;
        }

        iteration += 1;
        log.log(
            app,
            run_id,
            vec![(
                "stdout".to_string(),
                format!("--- Iteration {} ---", iteration),
            )],
        );
        emit_agent_iteration(app, run_id, iteration, "llm_call", None, total_tokens);

        let response = match call_llm_with_retry(
            provider.as_ref(),
            &llm_config,
            &messages,
            &tools,
            app,
            run_id,
            iteration,
            &log,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                log.flush_to_file(log_path);
                return Err(e);
            }
        };

        cumulative_input_tokens += response.usage.input_tokens;
        cumulative_output_tokens += response.usage.output_tokens;

        for block in &response.content {
            if let ContentBlock::Text { text } = block {
                let mut buf = log.buffer.lock().unwrap();
                buf.push(text.clone());
            }
        }

        if persist_run_metadata {
            update_run_metadata(
                db,
                run_id,
                iteration,
                cumulative_input_tokens,
                cumulative_output_tokens,
            );
        }

        match response.stop_reason {
            llm_provider::StopReason::EndTurn => {
                if finish_summary.is_none() {
                    finish_summary = extract_text_summary(&response.content);
                }
                let should_auto_continue = auto_continue_count < MAX_AUTO_CONTINUATIONS
                    && session_agent::should_auto_continue_after_end_turn(&response.content);
                messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: response.content.clone(),
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
                log.log(
                    app,
                    run_id,
                    vec![("stdout".to_string(), "\n[Agent ended turn]".to_string())],
                );
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
                        log.log(
                            app,
                            run_id,
                            vec![("stdout".to_string(), format!("\n[tool: {}]", name))],
                        );
                        emit_agent_iteration(
                            app,
                            run_id,
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
                            tool_ctx.as_ref(),
                            name,
                            input,
                            app,
                            run_id,
                            perm_reg,
                        )
                        .await
                        {
                            Ok((result, is_finish)) => {
                                let wrapped = format!(
                                    "<tool_result name=\"{}\" data_source=\"untrusted\">{}</tool_result>",
                                    name, result
                                );
                                tool_results.push(ContentBlock::ToolResult {
                                    tool_use_id: id.clone(),
                                    content: wrapped,
                                    is_error: false,
                                });
                                emit_agent_tool_result(app, run_id, iteration, id, &result, false);
                                if is_finish {
                                    finish_summary = Some(result);
                                    should_finish = true;
                                }
                            }
                            Err(err) => {
                                let err_content = format!("Error: {}", err);
                                log.log(
                                    app,
                                    run_id,
                                    vec![("stderr".to_string(), format!("[tool error: {}]", err))],
                                );
                                tool_results.push(ContentBlock::ToolResult {
                                    tool_use_id: id.clone(),
                                    content: err_content.clone(),
                                    is_error: true,
                                });
                                emit_agent_tool_result(
                                    app,
                                    run_id,
                                    iteration,
                                    id,
                                    &err_content,
                                    true,
                                );
                            }
                        }
                    }
                }

                messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: tool_results,
                    created_at: None,
                });

                if should_finish {
                    break;
                }
            }

            llm_provider::StopReason::MaxTokens => {
                let mut tool_error_results: Vec<ContentBlock> = Vec::new();
                for block in &response.content {
                    if let ContentBlock::ToolUse { id, name, .. } = block {
                        warn!(run_id = run_id, tool = %name, "tool_use truncated by max_tokens");
                        tool_error_results.push(ContentBlock::ToolResult {
                            tool_use_id: id.clone(),
                            content: "Error: your previous tool call was truncated because the response exceeded the token limit. Please retry with a shorter input, or break the work into smaller steps.".to_string(),
                            is_error: true,
                        });
                    }
                }

                messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: response.content,
                    created_at: None,
                });

                if !tool_error_results.is_empty() {
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
                log.log(
                    app,
                    run_id,
                    vec![(
                        "stderr".to_string(),
                        "[max_tokens reached, requesting continuation]".to_string(),
                    )],
                );
            }
        }
    }

    let total_tokens = cumulative_input_tokens + cumulative_output_tokens;
    emit_agent_iteration(app, run_id, iteration, "finished", None, total_tokens);

    log.log(
        app,
        run_id,
        vec![
            ("stdout".to_string(), "".to_string()),
            (
                "stdout".to_string(),
                "=== Agent Loop Complete ===".to_string(),
            ),
            (
                "stdout".to_string(),
                format!(
                    "Iterations: {} | Tokens: {} in + {} out = {}",
                    iteration, cumulative_input_tokens, cumulative_output_tokens, total_tokens
                ),
            ),
        ],
    );

    if let Some(ref summary) = finish_summary {
        log.log(
            app,
            run_id,
            vec![("stdout".to_string(), format!("Summary: {}", summary))],
        );
        if persist_run_metadata {
            if let Ok(conn) = db.get() {
                let _ = conn.execute(
                    "UPDATE runs SET metadata = json_set(COALESCE(metadata, '{}'), '$.finish_summary', ?1) WHERE id = ?2",
                    rusqlite::params![summary, run_id],
                );
            }
        }
    }

    let conversation_json = serde_json::to_string_pretty(&messages).unwrap_or_default();
    let memory_path = format!("memory/conversation_{}.json", run_id);
    let _ = workspace::write_workspace_file(agent_id, &memory_path, &conversation_json);

    save_conversation_to_db(
        db,
        agent_id,
        run_id,
        &conversation_json,
        cumulative_input_tokens,
        cumulative_output_tokens,
        iteration,
    );

    log.flush_to_file(log_path);

    let duration_ms = start.elapsed().as_millis() as i64;
    Ok(AgentLoopOutcome {
        finish_summary,
        iterations: iteration,
        input_tokens: cumulative_input_tokens,
        output_tokens: cumulative_output_tokens,
        duration_ms,
    })
}

pub async fn run_agent_loop(
    run_id: &str,
    agent_id: &str,
    cfg: &AgentLoopConfig,
    log_path: &PathBuf,
    _timeout_secs: u64,
    app: &tauri::AppHandle,
    mut cancel: oneshot::Receiver<()>,
    db: &DbPool,
    executor_tx: &tokio::sync::mpsc::UnboundedSender<crate::executor::engine::RunRequest>,
    chain_depth: i64,
    is_sub_agent: bool,
    agent_semaphores: &AgentSemaphores,
    session_registry: &SessionExecutionRegistry,
    permission_registry: &PermissionRegistry,
    memory_client: Option<&MemoryClient>,
    memory_user_id: &str,
    cloud_client: Option<std::sync::Arc<crate::db::cloud::SupabaseClient>>,
) -> Result<ProcessResult, String> {
    let outcome = execute_agent_loop_internal(
        run_id,
        agent_id,
        cfg,
        log_path,
        app,
        Some(&mut cancel),
        db,
        executor_tx,
        chain_depth,
        is_sub_agent,
        None,
        true,
        agent_semaphores,
        session_registry,
        permission_registry,
        memory_client,
        memory_user_id,
        cloud_client,
    )
    .await?;

    Ok(ProcessResult {
        exit_code: 0,
        duration_ms: outcome.duration_ms,
    })
}

pub async fn run_agent_loop_for_workflow(
    run_id: &str,
    agent_id: &str,
    cfg: &AgentLoopConfig,
    log_path: &PathBuf,
    app: &tauri::AppHandle,
    db: &DbPool,
    executor_tx: &tokio::sync::mpsc::UnboundedSender<crate::executor::engine::RunRequest>,
    project_id: Option<&str>,
    agent_semaphores: &AgentSemaphores,
    session_registry: &SessionExecutionRegistry,
    permission_registry: &PermissionRegistry,
    memory_client: Option<&MemoryClient>,
    memory_user_id: &str,
    cloud_client: Option<std::sync::Arc<crate::db::cloud::SupabaseClient>>,
) -> Result<AgentLoopOutcome, String> {
    execute_agent_loop_internal(
        run_id,
        agent_id,
        cfg,
        log_path,
        app,
        None,
        db,
        executor_tx,
        0,
        false,
        project_id,
        false,
        agent_semaphores,
        session_registry,
        permission_registry,
        memory_client,
        memory_user_id,
        cloud_client,
    )
    .await
}

// ─── Agent prompt (single-shot) ─────────────────────────────────────────────

/// Runs a single-shot prompt against the agent's configured LLM provider.
/// Unlike `run_agent_loop`, this makes exactly one LLM call with no tool use.
pub async fn run_agent_prompt(
    run_id: &str,
    agent_id: &str,
    cfg: &AgentStepConfig,
    log_path: &PathBuf,
    _timeout_secs: u64,
    app: &tauri::AppHandle,
    mut cancel: oneshot::Receiver<()>,
    db: &DbPool,
    _executor_tx: &tokio::sync::mpsc::UnboundedSender<crate::executor::engine::RunRequest>,
    _chain_depth: i64,
    _agent_semaphores: &AgentSemaphores,
    _session_registry: &SessionExecutionRegistry,
    memory_client: Option<&MemoryClient>,
    memory_user_id: &str,
    _cloud_client: Option<std::sync::Arc<crate::db::cloud::SupabaseClient>>,
) -> Result<ProcessResult, String> {
    let start = std::time::Instant::now();
    let log = AgentLog::new();

    // ── Load workspace config ────────────────────────────────────────────
    let ws_config = workspace::load_agent_config(agent_id).unwrap_or_default();
    let model = ws_config.model.clone();

    // ── Resolve provider + API key ───────────────────────────────────────
    let provider_name = &ws_config.provider;
    let api_key = keychain::retrieve_api_key(provider_name).map_err(|_| {
        let msg = format!(
            "No API key configured for provider '{}'. Set it in Settings.",
            provider_name
        );
        log.log(app, run_id, vec![("stderr".to_string(), msg.clone())]);
        log.flush_to_file(log_path);
        msg
    })?;

    let provider = llm_provider::create_provider(provider_name, api_key).map_err(|e| {
        log.log(app, run_id, vec![("stderr".to_string(), e.clone())]);
        log.flush_to_file(log_path);
        e
    })?;

    // ── Build context via pipeline ──────────────────────────────────────
    let pipeline = context::default_pipeline(memory_client.cloned());
    let allowed_tools = ContextRequest::effective_allowed_tools(&ws_config);
    let ctx_request = ContextRequest {
        agent_id: agent_id.to_string(),
        mode: ContextMode::SingleShot,
        session_id: None,
        session_type: None,
        project_id: None,
        goal: Some(cfg.prompt.clone()),
        ws_config: ws_config.clone(),
        allowed_tools,
        existing_messages: None,
        is_sub_agent: false,
        allow_sub_agents: true,
        chain_depth: 0,
        user_id: memory_user_id.to_string(),
    };
    let snapshot = pipeline.build(&ctx_request, db).await.map_err(|e| {
        log.log(app, run_id, vec![("stderr".to_string(), e.clone())]);
        log.flush_to_file(log_path);
        e
    })?;

    let llm_config = LlmConfig {
        model: model.clone(),
        max_tokens: DEFAULT_MAX_TOKENS_PER_CALL,
        temperature: Some(ws_config.temperature),
        system_prompt: snapshot.system_prompt,
    };

    log.log(
        app,
        run_id,
        vec![
            ("stdout".to_string(), "=== Prompt ===".to_string()),
            ("stdout".to_string(), cfg.prompt.clone()),
            ("stdout".to_string(), format!("Model: {}", model)),
            ("stdout".to_string(), "".to_string()),
        ],
    );

    // Check cancellation before calling LLM
    if cancel.try_recv().is_ok() {
        log.flush_to_file(log_path);
        return Err("cancelled".to_string());
    }

    let messages: Vec<ChatMessage> = if snapshot.messages.is_empty() {
        vec![ChatMessage {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: cfg.prompt.clone(),
            }],
            created_at: None,
        }]
    } else {
        snapshot.messages
    };

    emit_agent_iteration(app, run_id, 1, "llm_call", None, 0);

    // Single LLM call — no tools
    let response = match call_llm_with_retry(
        provider.as_ref(),
        &llm_config,
        &messages,
        &[],
        app,
        run_id,
        1,
        &log,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            log.log(
                app,
                run_id,
                vec![
                    ("stderr".to_string(), "".to_string()),
                    ("stderr".to_string(), format!("=== LLM Error ===\n{}", e)),
                ],
            );
            emit_agent_iteration(app, run_id, 1, "finished", None, 0);
            log.flush_to_file(log_path);
            return Err(e);
        }
    };

    let total_tokens = response.usage.input_tokens + response.usage.output_tokens;
    emit_agent_iteration(app, run_id, 1, "finished", None, total_tokens);

    // Extract text from response for log
    let response_text: String = response
        .content
        .iter()
        .filter_map(|block| {
            if let ContentBlock::Text { text } = block {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    log.log(
        app,
        run_id,
        vec![
            ("stdout".to_string(), "".to_string()),
            ("stdout".to_string(), "=== Response ===".to_string()),
            ("stdout".to_string(), response_text),
            ("stdout".to_string(), "".to_string()),
            (
                "stdout".to_string(),
                format!(
                    "Tokens: {} in + {} out = {}",
                    response.usage.input_tokens, response.usage.output_tokens, total_tokens
                ),
            ),
        ],
    );

    // Update run metadata
    update_run_metadata(
        db,
        run_id,
        1,
        response.usage.input_tokens,
        response.usage.output_tokens,
    );

    // Save conversation to DB
    let all_messages = vec![
        messages[0].clone(),
        ChatMessage {
            role: "assistant".to_string(),
            content: response.content,
            created_at: None,
        },
    ];
    let conversation_json = serde_json::to_string_pretty(&all_messages).unwrap_or_default();
    save_conversation_to_db(
        db,
        agent_id,
        run_id,
        &conversation_json,
        response.usage.input_tokens,
        response.usage.output_tokens,
        1,
    );

    // Write log file to disk
    log.flush_to_file(log_path);

    let duration_ms = start.elapsed().as_millis() as i64;
    Ok(ProcessResult {
        exit_code: 0,
        duration_ms,
    })
}

// ─── Pulse (chat-session-based with tool support) ─────────────────────────

const PULSE_MAX_ITERATIONS: u32 = 10;

/// Runs a pulse prompt: sends the goal as a user message into the agent's
/// dedicated "Pulse" chat session and runs the session loop with tool support.
pub async fn run_pulse(
    run_id: &str,
    agent_id: &str,
    goal: &str,
    log_path: &PathBuf,
    _timeout_secs: u64,
    app: &tauri::AppHandle,
    _cancel: oneshot::Receiver<()>,
    db: &DbPool,
    executor_tx: &tokio::sync::mpsc::UnboundedSender<crate::executor::engine::RunRequest>,
    chain_depth: i64,
    agent_semaphores: &AgentSemaphores,
    session_registry: &SessionExecutionRegistry,
    permission_registry: &PermissionRegistry,
    memory_client: Option<&MemoryClient>,
    memory_user_id: &str,
    cloud_client: Option<std::sync::Arc<crate::db::cloud::SupabaseClient>>,
) -> Result<ProcessResult, String> {
    let start = std::time::Instant::now();
    let log = AgentLog::new();
    let stream_id = format!("pulse:{}", agent_id);

    // ── Load workspace config ────────────────────────────────────────────
    let ws_config = workspace::load_agent_config(agent_id).unwrap_or_default();
    let max_iterations = if ws_config.max_iterations > 0 {
        ws_config.max_iterations.min(PULSE_MAX_ITERATIONS)
    } else {
        PULSE_MAX_ITERATIONS
    };
    let max_total_tokens = u32::MAX;

    let provider_name = &ws_config.provider;
    let api_key = keychain::retrieve_api_key(provider_name).map_err(|_| {
        let msg = format!("No API key for provider '{}'", provider_name);
        log.log(app, run_id, vec![("stderr".to_string(), msg.clone())]);
        log.flush_to_file(log_path);
        msg
    })?;

    // ── Find or create Pulse chat session + save user message ───────────
    let pool = db.0.clone();
    let aid = agent_id.to_string();
    let goal_text = goal.to_string();

    let session_id = tokio::task
    ::spawn_blocking({
      let pool = pool.clone();
      let aid = aid.clone();
      let goal_text = goal_text.clone();
      move || -> Result<String, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();

        // Find or create session
        let session_id: String = conn
          .query_row(
            "SELECT id FROM chat_sessions WHERE agent_id = ?1 AND session_type = 'pulse'",
            rusqlite::params![aid],
            |row| row.get(0)
          )
          .unwrap_or_else(|_| {
            let sid = ulid::Ulid::new().to_string();
            let _ = conn.execute(
              "INSERT INTO chat_sessions (
                 id, agent_id, title, archived, session_type, parent_session_id, source_bus_message_id,
                 chain_depth, execution_state, finish_summary, terminal_error, created_at, updated_at
               ) VALUES (?1, ?2, 'Pulse', 0, 'pulse', NULL, NULL, 0, 'running', NULL, NULL, ?3, ?3)",
              rusqlite::params![sid, aid, now]
            );
            sid
          });

        // Save user message (the pulse prompt)
        let msg_id = ulid::Ulid::new().to_string();
        let user_content = vec![ContentBlock::Text { text: goal_text.clone() }];
        let content_json = serde_json::to_string(&user_content).map_err(|e| e.to_string())?;

        conn
          .execute(
            "INSERT INTO chat_messages (id, session_id, role, content, created_at)
                 VALUES (?1, ?2, 'user', ?3, ?4)",
            rusqlite::params![msg_id, session_id, content_json, now]
          )
          .map_err(|e| e.to_string())?;

        conn
          .execute(
            "UPDATE chat_sessions SET updated_at = ?1, session_type = 'pulse', execution_state = 'running' WHERE id = ?2",
            rusqlite::params![now, session_id]
          )
          .map_err(|e| e.to_string())?;

        Ok(session_id)
      }
    }).await
    .map_err(|e| e.to_string())??;

    let pulse_project_id = session_worktree::load_session_project_id(db, &session_id).await?;
    if let Some(pid) = pulse_project_id.as_deref() {
        crate::commands::projects::assert_agent_in_project(db, pid, agent_id).await?;
        if let Err(e) = crate::executor::workspace::init_project_workspace(pid) {
            tracing::warn!(project_id = pid, "failed to init project workspace: {}", e);
        }
    }

    // ── Build context via pipeline ──────────────────────────────────────
    let pipeline = context::default_pipeline(memory_client.cloned());
    let allowed_tools = ContextRequest::effective_allowed_tools(&ws_config);
    let ctx_request = ContextRequest {
        agent_id: agent_id.to_string(),
        mode: ContextMode::Pulse,
        session_id: Some(session_id.clone()),
        session_type: Some("pulse".to_string()),
        project_id: pulse_project_id.clone(),
        goal: Some(goal.to_string()),
        ws_config: ws_config.clone(),
        allowed_tools,
        existing_messages: None,
        is_sub_agent: false,
        allow_sub_agents: true,
        chain_depth,
        user_id: memory_user_id.to_string(),
    };
    let snapshot = pipeline.build(&ctx_request, db).await.map_err(|e| {
        log.log(app, run_id, vec![("stderr".to_string(), e.clone())]);
        log.flush_to_file(log_path);
        e
    })?;

    let tools = snapshot.tools;
    let context_window = snapshot.token_budget.context_window;
    let llm_config = LlmConfig {
        model: ws_config.model.clone(),
        max_tokens: DEFAULT_MAX_TOKENS_PER_CALL,
        temperature: Some(ws_config.temperature),
        system_prompt: snapshot.system_prompt,
    };
    let history = snapshot.messages;

    log.log(
        app,
        run_id,
        vec![
            ("stdout".to_string(), "=== Pulse ===".to_string()),
            (
                "stdout".to_string(),
                format!("Model: {} | Tools: {}", ws_config.model, tools.len()),
            ),
            ("stdout".to_string(), "".to_string()),
        ],
    );

    // ── Build tool execution context ────────────────────────────────────
    let pulse_worktree = session_worktree::load_session_worktree_state(db, &session_id).await?;
    let tool_ctx = ToolExecutionContext::new_with_bus(
        agent_id,
        &stream_id,
        Some(&session_id),
        chain_depth,
        db.clone(),
        executor_tx.clone(),
        app.clone(),
        agent_semaphores.clone(),
        session_registry.clone(),
        pulse_worktree,
        pulse_project_id.as_deref(),
    )
    .with_permission_registry(permission_registry.clone())
    .with_memory_client(memory_client.cloned())
    .with_memory_user_id(memory_user_id.to_string())
    .with_cloud_client(cloud_client.clone());
    let tool_ctx = Arc::new(tool_ctx);

    // ── Create provider (wiring MCP bridge for CLI providers) ────────────
    let mcp_handle: Option<McpServerHandle> = app
        .try_state::<McpServerHandle>()
        .map(|s| s.inner().clone());
    let wiring = mcp_handle.map(|handle| AgentMcpWiring {
        handle,
        agent_id: agent_id.to_string(),
        run_id: run_id.to_string(),
        tool_ctx: tool_ctx.clone(),
        tools: tools.clone(),
        permission_registry: permission_registry.clone(),
        app: app.clone(),
        db: db.clone(),
    });
    let provider = llm_provider::create_provider_with_mcp(provider_name, api_key, wiring)
        .map_err(|e| {
            log.log(app, run_id, vec![("stderr".to_string(), e.clone())]);
            log.flush_to_file(log_path);
            e
        })?;

    // ── Run session loop (LLM + tool execution) ─────────────────────────
    let result = session_agent::run_session_loop(
        &provider,
        &llm_config,
        history,
        &tools,
        tool_ctx.as_ref(),
        &stream_id,
        &session_id,
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

    // ── Handle result ───────────────────────────────────────────────────
    match &result {
        Ok(summary) => {
            log.log(
                app,
                run_id,
                vec![
                    ("stdout".to_string(), "=== Pulse Complete ===".to_string()),
                    (
                        "stdout".to_string(),
                        format!("Session: {} | Summary: {}", session_id, summary),
                    ),
                ],
            );
        }
        Err(reason) if reason == "cancelled" => {
            session_agent::finalize_cancelled_session(db, &session_id).await;
            log.log(
                app,
                run_id,
                vec![("stderr".to_string(), "Pulse cancelled".to_string())],
            );
        }
        Err(reason) => {
            let _ = session_agent::finalize_failed_session(db, &session_id, reason).await;
            log.log(
                app,
                run_id,
                vec![("stderr".to_string(), format!("Pulse failed: {}", reason))],
            );
        }
    }

    // Save chat_session_id into run metadata
    if let Ok(conn) = db.get() {
        let metadata = serde_json::json!({
          "chat_session_id": session_id,
        });
        let _ = conn.execute(
            "UPDATE runs SET metadata = ?1 WHERE id = ?2",
            rusqlite::params![metadata.to_string(), run_id],
        );
    }

    log.flush_to_file(log_path);

    info!(run_id = run_id, agent_id = agent_id, "Pulse completed");

    let duration_ms = start.elapsed().as_millis() as i64;
    Ok(ProcessResult {
        exit_code: if result.is_ok() { 0 } else { 1 },
        duration_ms,
    })
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Call the LLM with retry logic (up to 3 attempts with exponential backoff).
async fn call_llm_with_retry(
    provider: &dyn LlmProvider,
    config: &LlmConfig,
    messages: &[ChatMessage],
    tools: &[ToolDefinition],
    app: &tauri::AppHandle,
    run_id: &str,
    iteration: u32,
    log: &AgentLog,
) -> Result<llm_provider::LlmResponse, String> {
    let mut last_error = String::new();

    for attempt in 0..LLM_RETRY_ATTEMPTS {
        match provider
            .chat_streaming(config, messages, tools, app, run_id, iteration)
            .await
        {
            Ok(response) => {
                return Ok(response);
            }
            Err(e) => {
                last_error = e.clone();
                if attempt < LLM_RETRY_ATTEMPTS - 1 {
                    let delay = LLM_RETRY_BASE_DELAY_MS * (1 << attempt);
                    warn!(
                        run_id = run_id,
                        attempt = attempt + 1,
                        error = %e,
                        delay_ms = delay,
                        "LLM call failed, retrying"
                    );
                    log.log(
                        app,
                        run_id,
                        vec![(
                            "stderr".to_string(),
                            format!(
                                "[LLM error (attempt {}/{}): {} — retrying in {}ms]",
                                attempt + 1,
                                LLM_RETRY_ATTEMPTS,
                                e,
                                delay
                            ),
                        )],
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                } else {
                    error!(run_id = run_id, error = %e, "LLM call failed after all retries");
                    log.log(
                        app,
                        run_id,
                        vec![(
                            "stderr".to_string(),
                            format!(
                                "[LLM error (attempt {}/{}): {}]",
                                attempt + 1,
                                LLM_RETRY_ATTEMPTS,
                                e
                            ),
                        )],
                    );
                }
            }
        }
    }

    Err(format!(
        "LLM call failed after {} attempts: {}",
        LLM_RETRY_ATTEMPTS, last_error
    ))
}

fn update_run_metadata(
    db: &DbPool,
    run_id: &str,
    iteration: u32,
    input_tokens: u32,
    output_tokens: u32,
) {
    if let Ok(conn) = db.get() {
        let metadata = serde_json::json!({
            "agent_loop": {
                "iteration": iteration,
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "total_tokens": input_tokens + output_tokens,
            }
        });
        let _ = conn.execute(
            "UPDATE runs SET metadata = ?1 WHERE id = ?2",
            rusqlite::params![metadata.to_string(), run_id],
        );
    }
}

fn save_conversation_to_db(
    db: &DbPool,
    agent_id: &str,
    run_id: &str,
    messages_json: &str,
    input_tokens: u32,
    output_tokens: u32,
    iterations: u32,
) {
    if let Ok(conn) = db.get() {
        let id = ulid::Ulid::new().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let _ = conn.execute(
      "INSERT INTO agent_conversations (id, agent_id, run_id, messages, total_input_tokens, total_output_tokens, iterations, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
      rusqlite::params![
        id,
        agent_id,
        run_id,
        messages_json,
        input_tokens,
        output_tokens,
        iterations,
        now
      ]
    );
    }
}

fn extract_text_summary(content: &[ContentBlock]) -> Option<String> {
    content.iter().rev().find_map(|block| match block {
        ContentBlock::Text { text } if !text.trim().is_empty() => Some(text.clone()),
        _ => None,
    })
}
