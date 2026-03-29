use std::path::PathBuf;
use std::sync::Mutex;
use tokio::sync::oneshot;
use tracing::{ error, info, warn };

use crate::db::DbPool;
use crate::events::emitter::{ emit_agent_iteration, emit_agent_tool_result, emit_log_chunk };
use crate::executor::agent_tools::{ self, ToolExecutionContext };
use crate::executor::keychain;
use crate::executor::llm_provider::{
  self,
  ChatMessage,
  ContentBlock,
  LlmConfig,
  LlmProvider,
  ToolDefinition,
};
use crate::executor::process::ProcessResult;
use crate::executor::workspace;
use crate::models::task::{ AgentLoopConfig, AgentStepConfig };

const DEFAULT_MAX_ITERATIONS: u32 = 25;
const DEFAULT_MAX_TOTAL_TOKENS: u32 = 200_000;
const DEFAULT_MAX_TOKENS_PER_CALL: u32 = 4096;
const LLM_RETRY_ATTEMPTS: u32 = 3;
const LLM_RETRY_BASE_DELAY_MS: u64 = 2000;

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

// ─── Agent loop ─────────────────────────────────────────────────────────────

pub async fn run_agent_loop(
  run_id: &str,
  agent_id: &str,
  cfg: &AgentLoopConfig,
  log_path: &PathBuf,
  _timeout_secs: u64,
  app: &tauri::AppHandle,
  mut cancel: oneshot::Receiver<()>,
  db: &DbPool
) -> Result<ProcessResult, String> {
  let start = std::time::Instant::now();
  let log = AgentLog::new();

  // ── Load workspace config ────────────────────────────────────────────
  let ws_config = workspace::load_agent_config(agent_id).unwrap_or_default();
  let model = cfg.model.clone().unwrap_or_else(|| ws_config.model.clone());
  let max_iterations = cfg.max_iterations.unwrap_or(
    if ws_config.max_iterations > 0 {
      ws_config.max_iterations
    } else {
      DEFAULT_MAX_ITERATIONS
    }
  );
  let max_total_tokens = cfg.max_total_tokens.unwrap_or(
    if ws_config.max_total_tokens > 0 {
      ws_config.max_total_tokens
    } else {
      DEFAULT_MAX_TOTAL_TOKENS
    }
  );

  // ── Load system prompt ───────────────────────────────────────────────
  let system_prompt = workspace
    ::read_workspace_file(agent_id, "system_prompt.md")
    .unwrap_or_else(|_| "You are a helpful autonomous agent.".to_string());

  // ── Resolve provider + API key ───────────────────────────────────────
  let provider_name = &ws_config.provider;
  let api_key = keychain::retrieve_api_key(provider_name).map_err(|_| {
    let msg =
      format!("No API key configured for provider '{}'. Set it in the Agent Config tab.", provider_name);
    log.log(app, run_id, vec![("stderr".to_string(), msg.clone())]);
    log.flush_to_file(log_path);
    msg
  })?;

  let provider = llm_provider::create_provider(provider_name, api_key).map_err(|e| {
    log.log(app, run_id, vec![("stderr".to_string(), e.clone())]);
    log.flush_to_file(log_path);
    e
  })?;

  let llm_config = LlmConfig {
    model,
    max_tokens: DEFAULT_MAX_TOKENS_PER_CALL,
    temperature: Some(ws_config.temperature),
    system_prompt,
  };

  // ── Build tool definitions ───────────────────────────────────────────
  let tools: Vec<ToolDefinition> = agent_tools::build_tool_definitions(&ws_config.allowed_tools);
  let tool_ctx = ToolExecutionContext::new(agent_id);

  // ── Apply template variable substitution to goal ─────────────────────
  let mut goal = cfg.goal.clone();
  if let Some(ref vars) = cfg.template_vars {
    for (key, value) in vars {
      goal = goal.replace(&format!("{{{{{}}}}}", key), value);
    }
  }

  // ── Init conversation ────────────────────────────────────────────────
  let mut messages: Vec<ChatMessage> = vec![ChatMessage {
    role: "user".to_string(),
    content: vec![ContentBlock::Text { text: goal.clone() }],
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
          llm_config.model,
          max_iterations,
          max_total_tokens
        ),
      ),
      ("stdout".to_string(), "".to_string())
    ]
  );

  let mut cumulative_input_tokens: u32 = 0;
  let mut cumulative_output_tokens: u32 = 0;
  let mut iteration: u32 = 0;
  let mut finish_summary: Option<String> = None;

  // ── Main loop ────────────────────────────────────────────────────────
  loop {
    // Check cancellation
    if cancel.try_recv().is_ok() {
      info!(run_id = run_id, "agent loop cancelled");
      log.flush_to_file(log_path);
      return Err("cancelled".to_string());
    }

    // Check iteration limit
    if iteration >= max_iterations {
      warn!(run_id = run_id, "agent loop hit iteration limit");
      log.log(
        app,
        run_id,
        vec![(
          "stderr".to_string(),
          format!("[iteration limit reached: {}/{}]", iteration, max_iterations),
        )]
      );
      // Ask for summary before breaking
      messages.push(ChatMessage {
        role: "user".to_string(),
        content: vec![ContentBlock::Text {
          text: "You have reached the maximum number of iterations. Please provide a final summary of what you accomplished and what remains to be done.".to_string(),
        }],
      });
      // One more LLM call for the summary
      if
        let Ok(resp) = call_llm_with_retry(
          provider.as_ref(),
          &llm_config,
          &messages,
          &[],
          app,
          run_id,
          iteration,
          &log
        ).await
      {
        cumulative_input_tokens += resp.usage.input_tokens;
        cumulative_output_tokens += resp.usage.output_tokens;
      }
      break;
    }

    // Check token budget
    let total_tokens = cumulative_input_tokens + cumulative_output_tokens;
    if total_tokens >= max_total_tokens {
      warn!(run_id = run_id, "agent loop hit token budget");
      log.log(
        app,
        run_id,
        vec![(
          "stderr".to_string(),
          format!("[token budget exceeded: {}/{}]", total_tokens, max_total_tokens),
        )]
      );
      break;
    }

    iteration += 1;
    log.log(app, run_id, vec![("stdout".to_string(), format!("--- Iteration {} ---", iteration))]);
    emit_agent_iteration(app, run_id, iteration, "llm_call", None, total_tokens);

    // ── LLM call ────────────────────────────────────────────────
    let response = match
      call_llm_with_retry(
        provider.as_ref(),
        &llm_config,
        &messages,
        &tools,
        app,
        run_id,
        iteration,
        &log
      ).await
    {
      Ok(r) => r,
      Err(e) => {
        log.flush_to_file(log_path);
        return Err(e);
      }
    };

    cumulative_input_tokens += response.usage.input_tokens;
    cumulative_output_tokens += response.usage.output_tokens;

    // Buffer the streamed LLM text into the log
    for block in &response.content {
      if let ContentBlock::Text { text } = block {
        let mut buf = log.buffer.lock().unwrap();
        buf.push(text.clone());
      }
    }

    // Update run metadata in DB
    update_run_metadata(db, run_id, iteration, cumulative_input_tokens, cumulative_output_tokens);

    // ── Handle response ─────────────────────────────────────────
    match response.stop_reason {
      llm_provider::StopReason::EndTurn => {
        messages.push(ChatMessage {
          role: "assistant".to_string(),
          content: response.content.clone(),
        });
        log.log(app, run_id, vec![("stdout".to_string(), "\n[Agent ended turn]".to_string())]);
        break;
      }

      llm_provider::StopReason::ToolUse => {
        messages.push(ChatMessage {
          role: "assistant".to_string(),
          content: response.content.clone(),
        });

        let mut tool_results: Vec<ContentBlock> = Vec::new();
        let mut should_finish = false;

        for block in &response.content {
          if let ContentBlock::ToolUse { id, name, input } = block {
            log.log(app, run_id, vec![("stdout".to_string(), format!("\n[tool: {}]", name))]);
            emit_agent_iteration(
              app,
              run_id,
              iteration,
              "tool_exec",
              Some(name),
              cumulative_input_tokens + cumulative_output_tokens
            );

            match agent_tools::execute_tool(&tool_ctx, name, input, app, run_id).await {
              Ok((result, is_finish)) => {
                tool_results.push(ContentBlock::ToolResult {
                  tool_use_id: id.clone(),
                  content: result.clone(),
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
                  vec![("stderr".to_string(), format!("[tool error: {}]", err))]
                );
                tool_results.push(ContentBlock::ToolResult {
                  tool_use_id: id.clone(),
                  content: err_content.clone(),
                  is_error: true,
                });
                emit_agent_tool_result(app, run_id, iteration, id, &err_content, true);
              }
            }
          }
        }

        messages.push(ChatMessage {
          role: "user".to_string(),
          content: tool_results,
        });

        if should_finish {
          break;
        }
      }

      llm_provider::StopReason::MaxTokens => {
        messages.push(ChatMessage {
          role: "assistant".to_string(),
          content: response.content,
        });
        messages.push(ChatMessage {
          role: "user".to_string(),
          content: vec![ContentBlock::Text {
            text: "Your response was cut off due to the token limit. Please continue where you left off.".to_string(),
          }],
        });
        log.log(
          app,
          run_id,
          vec![("stderr".to_string(), "[max_tokens reached, requesting continuation]".to_string())]
        );
      }
    }
  }

  // ── Save conversation to memory ──────────────────────────────────────
  let total_tokens = cumulative_input_tokens + cumulative_output_tokens;
  emit_agent_iteration(app, run_id, iteration, "finished", None, total_tokens);

  log.log(
    app,
    run_id,
    vec![
      ("stdout".to_string(), "".to_string()),
      ("stdout".to_string(), "=== Agent Loop Complete ===".to_string()),
      (
        "stdout".to_string(),
        format!(
          "Iterations: {} | Tokens: {} in + {} out = {}",
          iteration,
          cumulative_input_tokens,
          cumulative_output_tokens,
          total_tokens
        ),
      )
    ]
  );

  if let Some(ref summary) = finish_summary {
    log.log(app, run_id, vec![("stdout".to_string(), format!("Summary: {}", summary))]);
  }

  // Persist conversation history
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
    iteration
  );

  // Write log file to disk
  log.flush_to_file(log_path);

  let duration_ms = start.elapsed().as_millis() as i64;
  Ok(ProcessResult {
    exit_code: 0,
    duration_ms,
  })
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
  db: &DbPool
) -> Result<ProcessResult, String> {
  let start = std::time::Instant::now();
  let log = AgentLog::new();

  // ── Load workspace config ────────────────────────────────────────────
  let ws_config = workspace::load_agent_config(agent_id).unwrap_or_default();
  let model = ws_config.model.clone();

  // ── Load system prompt ───────────────────────────────────────────────
  let system_prompt = workspace
    ::read_workspace_file(agent_id, "system_prompt.md")
    .unwrap_or_else(|_| "You are a helpful assistant.".to_string());

  // ── Resolve provider + API key ───────────────────────────────────────
  let provider_name = &ws_config.provider;
  let api_key = keychain::retrieve_api_key(provider_name).map_err(|_| {
    let msg =
      format!("No API key configured for provider '{}'. Set it in the Agent Config tab.", provider_name);
    log.log(app, run_id, vec![("stderr".to_string(), msg.clone())]);
    log.flush_to_file(log_path);
    msg
  })?;

  let provider = llm_provider::create_provider(provider_name, api_key).map_err(|e| {
    log.log(app, run_id, vec![("stderr".to_string(), e.clone())]);
    log.flush_to_file(log_path);
    e
  })?;

  let llm_config = LlmConfig {
    model: model.clone(),
    max_tokens: DEFAULT_MAX_TOKENS_PER_CALL,
    temperature: Some(ws_config.temperature),
    system_prompt,
  };

  log.log(
    app,
    run_id,
    vec![
      ("stdout".to_string(), "=== Prompt ===".to_string()),
      ("stdout".to_string(), cfg.prompt.clone()),
      ("stdout".to_string(), format!("Model: {}", model)),
      ("stdout".to_string(), "".to_string())
    ]
  );

  // Check cancellation before calling LLM
  if cancel.try_recv().is_ok() {
    log.flush_to_file(log_path);
    return Err("cancelled".to_string());
  }

  let messages: Vec<ChatMessage> = vec![ChatMessage {
    role: "user".to_string(),
    content: vec![ContentBlock::Text {
      text: cfg.prompt.clone(),
    }],
  }];

  emit_agent_iteration(app, run_id, 1, "llm_call", None, 0);

  // Single LLM call — no tools
  let response = match
    call_llm_with_retry(provider.as_ref(), &llm_config, &messages, &[], app, run_id, 1, &log).await
  {
    Ok(r) => r,
    Err(e) => {
      log.log(
        app,
        run_id,
        vec![
          ("stderr".to_string(), "".to_string()),
          ("stderr".to_string(), format!("=== LLM Error ===\n{}", e))
        ]
      );
      emit_agent_iteration(app, run_id, 1, "finished", None, 0);
      log.flush_to_file(log_path);
      return Err(e);
    }
  };

  let total_tokens = response.usage.input_tokens + response.usage.output_tokens;
  emit_agent_iteration(app, run_id, 1, "finished", None, total_tokens);

  // Extract text from response for log
  let response_text: String = response.content
    .iter()
    .filter_map(|block| {
      if let ContentBlock::Text { text } = block { Some(text.as_str()) } else { None }
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
          response.usage.input_tokens,
          response.usage.output_tokens,
          total_tokens
        ),
      )
    ]
  );

  // Update run metadata
  update_run_metadata(db, run_id, 1, response.usage.input_tokens, response.usage.output_tokens);

  // Save conversation to DB
  let all_messages = vec![messages[0].clone(), ChatMessage {
    role: "assistant".to_string(),
    content: response.content,
  }];
  let conversation_json = serde_json::to_string_pretty(&all_messages).unwrap_or_default();
  save_conversation_to_db(
    db,
    agent_id,
    run_id,
    &conversation_json,
    response.usage.input_tokens,
    response.usage.output_tokens,
    1
  );

  // Write log file to disk
  log.flush_to_file(log_path);

  let duration_ms = start.elapsed().as_millis() as i64;
  Ok(ProcessResult {
    exit_code: 0,
    duration_ms,
  })
}

// ─── Pulse (chat-session-based single-shot) ────────────────────────────────

/// Runs a pulse prompt: sends the goal as a user message into the agent's
/// dedicated "Pulse" chat session and streams the LLM response back into it.
pub async fn run_pulse(
  run_id: &str,
  agent_id: &str,
  goal: &str,
  log_path: &PathBuf,
  _timeout_secs: u64,
  app: &tauri::AppHandle,
  _cancel: oneshot::Receiver<()>,
  db: &DbPool
) -> Result<ProcessResult, String> {
  let start = std::time::Instant::now();
  let log = AgentLog::new();
  let stream_id = format!("pulse:{}", agent_id);

  // ── Load workspace config ────────────────────────────────────────────
  let ws_config = workspace::load_agent_config(agent_id).unwrap_or_default();
  let system_prompt = workspace
    ::read_workspace_file(agent_id, "system_prompt.md")
    .unwrap_or_else(|_| "You are a helpful assistant.".to_string());

  let provider_name = &ws_config.provider;
  let api_key = keychain::retrieve_api_key(provider_name).map_err(|_| {
    let msg = format!("No API key for provider '{}'", provider_name);
    log.log(app, run_id, vec![("stderr".to_string(), msg.clone())]);
    log.flush_to_file(log_path);
    msg
  })?;

  let provider = llm_provider::create_provider(provider_name, api_key).map_err(|e| {
    log.log(app, run_id, vec![("stderr".to_string(), e.clone())]);
    log.flush_to_file(log_path);
    e
  })?;

  let llm_config = LlmConfig {
    model: ws_config.model.clone(),
    max_tokens: DEFAULT_MAX_TOKENS_PER_CALL,
    temperature: Some(ws_config.temperature),
    system_prompt,
  };

  // ── Find or create Pulse chat session ────────────────────────────────
  let pool = db.0.clone();
  let aid = agent_id.to_string();
  let goal_text = goal.to_string();

  let (session_id, history) = tokio::task
    ::spawn_blocking({
      let pool = pool.clone();
      let aid = aid.clone();
      let goal_text = goal_text.clone();
      move || -> Result<(String, Vec<ChatMessage>), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();

        // Find or create session
        let session_id: String = conn
          .query_row(
            "SELECT id FROM chat_sessions WHERE agent_id = ?1 AND title = 'Pulse'",
            rusqlite::params![aid],
            |row| row.get(0)
          )
          .unwrap_or_else(|_| {
            let sid = ulid::Ulid::new().to_string();
            let _ = conn.execute(
              "INSERT INTO chat_sessions (id, agent_id, title, archived, created_at, updated_at)
                         VALUES (?1, ?2, 'Pulse', 0, ?3, ?3)",
              rusqlite::params![sid, aid, now]
            );
            sid
          });

        // Load existing messages
        let mut stmt = conn
          .prepare(
            "SELECT role, content FROM chat_messages
                     WHERE session_id = ?1 ORDER BY created_at ASC"
          )
          .map_err(|e| e.to_string())?;

        let mut messages: Vec<ChatMessage> = stmt
          .query_map(rusqlite::params![session_id], |row| {
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
            "UPDATE chat_sessions SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now, session_id]
          )
          .map_err(|e| e.to_string())?;

        messages.push(ChatMessage {
          role: "user".to_string(),
          content: user_content,
        });

        Ok((session_id, messages))
      }
    }).await
    .map_err(|e| e.to_string())??;

  log.log(
    app,
    run_id,
    vec![
      ("stdout".to_string(), "=== Pulse ===".to_string()),
      ("stdout".to_string(), format!("Model: {}", ws_config.model)),
      ("stdout".to_string(), "".to_string())
    ]
  );

  emit_agent_iteration(app, &stream_id, 1, "llm_call", None, 0);

  // ── LLM call ─────────────────────────────────────────────────────────
  let response = match
    call_llm_with_retry(
      provider.as_ref(),
      &llm_config,
      &history,
      &[],
      app,
      &stream_id,
      1,
      &log
    ).await
  {
    Ok(r) => r,
    Err(e) => {
      log.log(app, run_id, vec![("stderr".to_string(), format!("=== Pulse Error ===\n{}", e))]);
      emit_agent_iteration(app, &stream_id, 1, "finished", None, 0);
      log.flush_to_file(log_path);
      return Err(e);
    }
  };

  let total_tokens = response.usage.input_tokens + response.usage.output_tokens;
  emit_agent_iteration(app, &stream_id, 1, "finished", None, total_tokens);

  // ── Save assistant response to chat session ──────────────────────────
  let content_json = serde_json::to_string(&response.content).map_err(|e| e.to_string())?;
  let sid = session_id.clone();

  tokio::task
    ::spawn_blocking({
      let pool = pool.clone();
      move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let msg_id = ulid::Ulid::new().to_string();
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
    }).await
    .map_err(|e| e.to_string())??;

  log.log(
    app,
    run_id,
    vec![
      ("stdout".to_string(), "=== Pulse Complete ===".to_string()),
      ("stdout".to_string(), format!("Tokens: {} | Session: {}", total_tokens, session_id))
    ]
  );

  log.flush_to_file(log_path);

  info!(run_id = run_id, agent_id = agent_id, "Pulse completed ({} tokens)", total_tokens);

  let duration_ms = start.elapsed().as_millis() as i64;
  Ok(ProcessResult {
    exit_code: 0,
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
  log: &AgentLog
) -> Result<llm_provider::LlmResponse, String> {
  let mut last_error = String::new();

  for attempt in 0..LLM_RETRY_ATTEMPTS {
    match provider.chat_streaming(config, messages, tools, app, run_id, iteration).await {
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
            )]
          );
          tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
        } else {
          error!(run_id = run_id, error = %e, "LLM call failed after all retries");
          log.log(
            app,
            run_id,
            vec![(
              "stderr".to_string(),
              format!("[LLM error (attempt {}/{}): {}]", attempt + 1, LLM_RETRY_ATTEMPTS, e),
            )]
          );
        }
      }
    }
  }

  Err(format!("LLM call failed after {} attempts: {}", LLM_RETRY_ATTEMPTS, last_error))
}

fn update_run_metadata(
  db: &DbPool,
  run_id: &str,
  iteration: u32,
  input_tokens: u32,
  output_tokens: u32
) {
  if let Ok(conn) = db.get() {
    let metadata =
      serde_json::json!({
            "agent_loop": {
                "iteration": iteration,
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "total_tokens": input_tokens + output_tokens,
            }
        });
    let _ = conn.execute(
      "UPDATE runs SET metadata = ?1 WHERE id = ?2",
      rusqlite::params![metadata.to_string(), run_id]
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
  iterations: u32
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
