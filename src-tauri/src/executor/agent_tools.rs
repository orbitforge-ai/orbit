use serde_json::json;
use std::path::{ Path, PathBuf };
use tokio::sync::mpsc;
use tracing::{ info, warn };

use crate::db::DbPool;
use crate::events::emitter::{ emit_bus_message_sent, emit_log_chunk, emit_sub_agents_spawned };
use crate::executor::engine::RunRequest;
use crate::executor::llm_provider::ToolDefinition;
use crate::executor::skills;
use crate::models::task::{ AgentLoopConfig, Task };

/// Maximum chain depth before agent bus rejects further sends.
const MAX_CHAIN_DEPTH: i64 = 10;
/// Maximum number of sub-agents per spawn call.
const MAX_SUB_AGENTS: usize = 10;
/// Default timeout for sub-agent execution in seconds.
const DEFAULT_SUB_AGENT_TIMEOUT_SECS: u64 = 300;
/// Maximum timeout for sub-agent execution in seconds.
const MAX_SUB_AGENT_TIMEOUT_SECS: u64 = 600;

/// Context for executing agent tools — provides sandboxed filesystem access
/// and optional Agent Bus capabilities.
pub struct ToolExecutionContext {
  /// The agent's ID (used for skill discovery and other lookups).
  pub agent_id: String,
  /// The agent's entire root directory (~/.orbit/agents/{agent_id}/).
  pub _agent_root: PathBuf,
  /// The workspace subdirectory for scratch files.
  pub workspace_root: PathBuf,
  /// Which search provider to use for web_search (e.g. "brave", "tavily").
  pub web_search_provider: String,
  /// Skills explicitly disabled for this agent.
  pub disabled_skills: Vec<String>,
  // ─── Agent Bus fields ───────────────────────────────────────────────
  pub db: Option<DbPool>,
  pub executor_tx: Option<mpsc::UnboundedSender<RunRequest>>,
  pub app: Option<tauri::AppHandle>,
  pub current_agent_id: Option<String>,
  pub current_run_id: Option<String>,
  pub chain_depth: i64,
  /// Whether this context is for a sub-agent (prevents nesting).
  pub is_sub_agent: bool,
}

impl ToolExecutionContext {
  pub fn new(agent_id: &str) -> Self {
    let agent_root = super::workspace::agent_dir(agent_id);
    let workspace_root = agent_root.join("workspace");
    let ws_config = super::workspace::load_agent_config(agent_id).unwrap_or_default();
    Self {
      agent_id: agent_id.to_string(),
      _agent_root: agent_root,
      workspace_root,
      web_search_provider: ws_config.web_search_provider,
      disabled_skills: ws_config.disabled_skills,
      db: None,
      executor_tx: None,
      app: None,
      current_agent_id: None,
      current_run_id: None,
      chain_depth: 0,
      is_sub_agent: false,
    }
  }

  pub fn new_with_bus(
    agent_id: &str,
    run_id: &str,
    chain_depth: i64,
    db: DbPool,
    executor_tx: mpsc::UnboundedSender<RunRequest>,
    app: tauri::AppHandle,
  ) -> Self {
    let agent_root = super::workspace::agent_dir(agent_id);
    let workspace_root = agent_root.join("workspace");
    let ws_config = super::workspace::load_agent_config(agent_id).unwrap_or_default();
    Self {
      agent_id: agent_id.to_string(),
      _agent_root: agent_root,
      workspace_root,
      web_search_provider: ws_config.web_search_provider,
      disabled_skills: ws_config.disabled_skills,
      db: Some(db),
      executor_tx: Some(executor_tx),
      app: Some(app),
      current_agent_id: Some(agent_id.to_string()),
      current_run_id: Some(run_id.to_string()),
      chain_depth,
      is_sub_agent: false,
    }
  }

  pub fn new_for_sub_agent(
    agent_id: &str,
    run_id: &str,
    chain_depth: i64,
    db: DbPool,
    executor_tx: mpsc::UnboundedSender<RunRequest>,
    app: tauri::AppHandle,
  ) -> Self {
    let mut ctx = Self::new_with_bus(agent_id, run_id, chain_depth, db, executor_tx, app);
    ctx.is_sub_agent = true;
    ctx
  }
}

/// Build the tool definitions that are exposed to the LLM.
pub fn build_tool_definitions(allowed: &[String]) -> Vec<ToolDefinition> {
  let all_tools = vec![
    ToolDefinition {
      name: "shell_command".to_string(),
      description: "Execute a shell command in the agent's workspace directory. Returns stdout and stderr.".to_string(),
      input_schema: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    }
                },
                "required": ["command"]
            }),
    },
    ToolDefinition {
      name: "read_file".to_string(),
      description: "Read the contents of a file from the agent's workspace. Path is relative to the workspace directory.".to_string(),
      input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to the file within the workspace"
                    }
                },
                "required": ["path"]
            }),
    },
    ToolDefinition {
      name: "write_file".to_string(),
      description: "Write content to a file in the agent's workspace. Creates parent directories if needed. Path is relative to the workspace directory.".to_string(),
      input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to the file within the workspace"
                    },
                    "content": {
                        "type": "string",
                        "description": "The content to write to the file"
                    }
                },
                "required": ["path", "content"]
            }),
    },
    ToolDefinition {
      name: "list_files".to_string(),
      description: "List files and directories in the agent's workspace. Path is relative to the workspace directory.".to_string(),
      input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to the directory within the workspace. Use '.' for the workspace root."
                    }
                },
                "required": ["path"]
            }),
    },
    ToolDefinition {
      name: "web_search".to_string(),
      description: "Search the web for information. Returns a list of results with titles, URLs, and descriptions.".to_string(),
      input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    },
                    "count": {
                        "type": "integer",
                        "description": "Number of results to return (default: 5, max: 10)"
                    }
                },
                "required": ["query"]
            }),
    },
    ToolDefinition {
      name: "send_message".to_string(),
      description: "Send a message to another agent, triggering it to run with your message as its goal. Use this to delegate work or coordinate with other agents.".to_string(),
      input_schema: json!({
                "type": "object",
                "properties": {
                    "target_agent": {
                        "type": "string",
                        "description": "The name or ID of the agent to send the message to"
                    },
                    "message": {
                        "type": "string",
                        "description": "The message/instructions for the target agent"
                    },
                    "wait_for_result": {
                        "type": "boolean",
                        "description": "If true, wait for the target agent to complete and return its result. Default: false (fire-and-forget)."
                    }
                },
                "required": ["target_agent", "message"]
            }),
    },
    ToolDefinition {
      name: "activate_skill".to_string(),
      description: "Activate a skill to load its full instructions into context. Use this when a task matches one of the skills listed in <available-skills>. Pass the skill name exactly as shown.".to_string(),
      input_schema: json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "The name of the skill to activate (from <available-skills>)"
                    }
                },
                "required": ["skill_name"]
            }),
    },
    ToolDefinition {
      name: "spawn_sub_agents".to_string(),
      description: "Break down work into parallel sub-tasks. Each sub-task runs as an independent agent loop with its own context. All sub-tasks execute concurrently and their results are returned together. Sub-agents have access to all your tools and the shared workspace, but cannot spawn further sub-agents.".to_string(),
      input_schema: json!({
                "type": "object",
                "properties": {
                    "tasks": {
                        "type": "array",
                        "description": "Array of sub-tasks to execute concurrently",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {
                                    "type": "string",
                                    "description": "A short identifier for this sub-task (e.g., 'research', 'write-tests')"
                                },
                                "goal": {
                                    "type": "string",
                                    "description": "The goal/instructions for this sub-agent"
                                }
                            },
                            "required": ["id", "goal"]
                        },
                        "minItems": 1,
                        "maxItems": 10
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "description": "Per-sub-agent timeout in seconds (default: 300, max: 600). If a sub-agent exceeds this, it is cancelled."
                    }
                },
                "required": ["tasks"]
            }),
    },
    ToolDefinition {
      name: "finish".to_string(),
      description: "Signal that the goal has been completed. Provide a summary of what was accomplished.".to_string(),
      input_schema: json!({
                "type": "object",
                "properties": {
                    "summary": {
                        "type": "string",
                        "description": "A summary of what was accomplished"
                    }
                },
                "required": ["summary"]
            }),
    }
  ];

  if allowed.is_empty() {
    return all_tools;
  }

  all_tools
    .into_iter()
    .filter(|t| allowed.contains(&t.name))
    .collect()
}

/// Validate a path stays within the given base directory.
fn validate_path(base: &Path, requested: &str) -> Result<PathBuf, String> {
  let resolved = base.join(requested);

  if resolved.exists() {
    let canonical = resolved.canonicalize().map_err(|e| format!("failed to resolve path: {}", e))?;
    let base_canonical = base.canonicalize().map_err(|e| format!("failed to resolve base: {}", e))?;
    if !canonical.starts_with(&base_canonical) {
      return Err(format!("path escapes workspace: {}", requested));
    }
    return Ok(canonical);
  }

  // For new files, validate the parent
  let parent = resolved.parent().ok_or("invalid path")?;
  if !parent.exists() {
    std::fs::create_dir_all(parent).map_err(|e| format!("failed to create directories: {}", e))?;
  }
  let parent_canonical = parent
    .canonicalize()
    .map_err(|e| format!("failed to resolve parent: {}", e))?;
  let base_canonical = base.canonicalize().map_err(|e| format!("failed to resolve base: {}", e))?;
  if !parent_canonical.starts_with(&base_canonical) {
    return Err(format!("path escapes workspace: {}", requested));
  }

  Ok(parent_canonical.join(resolved.file_name().ok_or("no filename")?))
}

/// Execute a single tool call. Returns (result_text, is_finish).
pub async fn execute_tool(
  ctx: &ToolExecutionContext,
  tool_name: &str,
  input: &serde_json::Value,
  app: &tauri::AppHandle,
  run_id: &str
) -> Result<(String, bool), String> {
  match tool_name {
    "shell_command" => {
      let command = input["command"].as_str().ok_or("shell_command: missing 'command' field")?;

      info!(run_id = run_id, command = command, "agent tool: shell_command");

      // Ensure workspace dir exists
      std::fs
        ::create_dir_all(&ctx.workspace_root)
        .map_err(|e| format!("failed to create workspace: {}", e))?;

      let output = tokio::process::Command
        ::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .current_dir(&ctx.workspace_root)
        .output().await
        .map_err(|e| format!("failed to execute command: {}", e))?;

      let stdout = String::from_utf8_lossy(&output.stdout).to_string();
      let stderr = String::from_utf8_lossy(&output.stderr).to_string();
      let exit_code = output.status.code().unwrap_or(-1);

      // Emit log chunks
      if !stdout.is_empty() {
        let lines: Vec<(String, String)> = stdout
          .lines()
          .map(|l| ("stdout".to_string(), l.to_string()))
          .collect();
        emit_log_chunk(app, run_id, lines);
      }
      if !stderr.is_empty() {
        let lines: Vec<(String, String)> = stderr
          .lines()
          .map(|l| ("stderr".to_string(), l.to_string()))
          .collect();
        emit_log_chunk(app, run_id, lines);
      }

      let mut result = String::new();
      if !stdout.is_empty() {
        result.push_str(&stdout);
      }
      if !stderr.is_empty() {
        if !result.is_empty() {
          result.push('\n');
        }
        result.push_str("[stderr]\n");
        result.push_str(&stderr);
      }
      result.push_str(&format!("\n[exit code: {}]", exit_code));

      // Truncate to 50KB to avoid blowing up context
      if result.len() > 50_000 {
        result.truncate(50_000);
        result.push_str("\n[output truncated]");
      }

      Ok((result, false))
    }

    "read_file" => {
      let path = input["path"].as_str().ok_or("read_file: missing 'path' field")?;

      let full_path = validate_path(&ctx.workspace_root, path)?;
      let content = std::fs
        ::read_to_string(&full_path)
        .map_err(|e| format!("failed to read {}: {}", path, e))?;

      // Truncate to 100KB
      let content = if content.len() > 100_000 {
        let mut truncated = content[..100_000].to_string();
        truncated.push_str("\n[file truncated at 100KB]");
        truncated
      } else {
        content
      };

      Ok((content, false))
    }

    "write_file" => {
      let path = input["path"].as_str().ok_or("write_file: missing 'path' field")?;
      let content = input["content"].as_str().ok_or("write_file: missing 'content' field")?;

      let full_path = validate_path(&ctx.workspace_root, path)?;
      if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("failed to create dirs: {}", e))?;
      }
      std::fs::write(&full_path, content).map_err(|e| format!("failed to write {}: {}", path, e))?;

      Ok((format!("Successfully wrote {} bytes to {}", content.len(), path), false))
    }

    "list_files" => {
      let path = input["path"].as_str().ok_or("list_files: missing 'path' field")?;

      let full_path = validate_path(&ctx.workspace_root, path)?;
      if !full_path.is_dir() {
        return Err(format!("{} is not a directory", path));
      }

      let entries = std::fs
        ::read_dir(&full_path)
        .map_err(|e| format!("failed to list {}: {}", path, e))?;

      let mut listing = Vec::new();
      for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let meta = entry.metadata().map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().to_string();
        let kind = if meta.is_dir() { "dir" } else { "file" };
        let size = meta.len();
        listing.push(format!("{:>6} {:4} {}", size, kind, name));
      }
      listing.sort();

      if listing.is_empty() {
        Ok(("(empty directory)".to_string(), false))
      } else {
        Ok((listing.join("\n"), false))
      }
    }

    "web_search" => {
      let query = input["query"].as_str().ok_or("web_search: missing 'query' field")?;
      let count = input["count"].as_u64().unwrap_or(5).min(10) as u32;

      info!(run_id = run_id, query = query, provider = %ctx.web_search_provider, "agent tool: web_search");

      let result = execute_web_search(&ctx.web_search_provider, query, count).await?;

      Ok((result, false))
    }

    "send_message" => {
      let target = input["target_agent"].as_str().ok_or("send_message: missing 'target_agent' field")?;
      let message = input["message"].as_str().ok_or("send_message: missing 'message' field")?;
      let wait = input["wait_for_result"].as_bool().unwrap_or(false);

      info!(run_id = run_id, target = target, wait = wait, "agent tool: send_message");

      let db = ctx.db.as_ref().ok_or("send_message: agent bus not available in this context")?;
      let tx = ctx.executor_tx.as_ref().ok_or("send_message: executor channel not available")?;
      let bus_app = ctx.app.as_ref().ok_or("send_message: app handle not available")?;
      let from_agent = ctx.current_agent_id.as_deref().unwrap_or("unknown");
      let from_run = ctx.current_run_id.as_deref().unwrap_or("unknown");

      // Check chain depth
      let next_depth = ctx.chain_depth + 1;
      if next_depth > MAX_CHAIN_DEPTH {
        return Ok((
          format!("Error: Maximum chain depth ({}) exceeded. Cannot trigger further agents to prevent infinite loops.", MAX_CHAIN_DEPTH),
          false,
        ));
      }

      // Resolve target agent by name or ID
      let (to_agent_id, to_agent_name) = {
        let pool = db.clone();
        let target_str = target.to_string();
        tokio::task::spawn_blocking(move || {
          let conn = pool.get().map_err(|e| e.to_string())?;
          // Try by ID first, then by name
          let result: Result<(String, String), String> = conn
            .query_row(
              "SELECT id, name FROM agents WHERE id = ?1 OR name = ?1 LIMIT 1",
              rusqlite::params![target_str],
              |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(|_| format!("Agent '{}' not found", target_str));
          result
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?
      };

      // Truncate payload to 50KB
      let payload_str = if message.len() > 50_000 {
        &message[..50_000]
      } else {
        message
      };

      let msg_id = ulid::Ulid::new().to_string();
      let new_run_id = ulid::Ulid::new().to_string();
      let now = chrono::Utc::now().to_rfc3339();

      // Create an ephemeral agent_loop task for the target agent
      let task_id = ulid::Ulid::new().to_string();
      let loop_config = AgentLoopConfig {
        goal: payload_str.to_string(),
        model: None,
        max_iterations: None,
        max_total_tokens: None,
        template_vars: None,
      };
      let config_json = serde_json::to_value(&loop_config).map_err(|e| e.to_string())?;

      {
        let pool = db.clone();
        let task_id = task_id.clone();
        let to_agent_id = to_agent_id.clone();
        let config_json_str = config_json.to_string();
        let now = now.clone();
        tokio::task::spawn_blocking(move || {
          let conn = pool.get().map_err(|e| e.to_string())?;
          conn.execute(
            "INSERT INTO tasks (id, name, kind, config, max_duration_seconds, max_retries, retry_delay_seconds, concurrency_policy, tags, agent_id, session_id, enabled, created_at, updated_at)
             VALUES (?1, ?2, 'agent_loop', ?3, 300, 0, 0, 'allow', '[]', ?4, NULL, 1, ?5, ?5)",
            rusqlite::params![task_id, format!("bus:{}", task_id), config_json_str, to_agent_id, now],
          ).map_err(|e| e.to_string())?;
          Ok::<(), String>(())
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
      }

      // Insert bus message and run records
      {
        let pool = db.clone();
        let msg_id = msg_id.clone();
        let from_agent = from_agent.to_string();
        let from_run = from_run.to_string();
        let to_agent_id = to_agent_id.clone();
        let new_run_id = new_run_id.clone();
        let payload_str = payload_str.to_string();
        let task_id = task_id.clone();
        let now = now.clone();
        let log_path = format!(
          "{}/.orbit/logs/{}.log",
          std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
          new_run_id
        );

        tokio::task::spawn_blocking(move || {
          let conn = pool.get().map_err(|e| e.to_string())?;

          // Insert bus message
          conn.execute(
            "INSERT INTO bus_messages (id, from_agent_id, from_run_id, to_agent_id, to_run_id, kind, payload, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'direct', ?6, 'delivered', ?7)",
            rusqlite::params![msg_id, from_agent, from_run, to_agent_id, new_run_id, payload_str, now],
          ).map_err(|e| e.to_string())?;

          // Insert run record
          conn.execute(
            "INSERT INTO runs (id, task_id, schedule_id, agent_id, state, trigger, log_path, retry_count, parent_run_id, metadata, chain_depth, source_bus_message_id, created_at)
             VALUES (?1, ?2, NULL, ?3, 'pending', 'bus', ?4, 0, NULL, '{}', ?5, ?6, ?7)",
            rusqlite::params![new_run_id, task_id, to_agent_id, log_path, next_depth, msg_id, now],
          ).map_err(|e| e.to_string())?;

          Ok::<(), String>(())
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
      }

      // Build a Task struct for the RunRequest
      let task = Task {
        id: task_id,
        name: format!("bus-message-{}", msg_id),
        description: Some(format!("Bus message from agent '{}'", from_agent)),
        kind: "agent_loop".to_string(),
        config: config_json,
        max_duration_seconds: 300,
        max_retries: 0,
        retry_delay_seconds: 0,
        concurrency_policy: "allow".to_string(),
        tags: vec![],
        agent_id: Some(to_agent_id.clone()),
        session_id: None,
        enabled: true,
        created_at: now.clone(),
        updated_at: now.clone(),
      };

      // Send RunRequest to executor
      let req = RunRequest {
        run_id: new_run_id.clone(),
        task,
        schedule_id: None,
        _trigger: "bus".to_string(),
        retry_count: 0,
        _parent_run_id: None,
        chain_depth: next_depth,
      };
      tx.send(req).map_err(|e| format!("failed to enqueue run: {}", e))?;

      // Emit bus event
      emit_bus_message_sent(
        bus_app,
        &msg_id,
        from_agent,
        &to_agent_id,
        "direct",
        json!({ "message": payload_str }),
        &new_run_id,
      );

      if !wait {
        return Ok((
          format!("Message sent to agent '{}'. Triggered run ID: {}. The agent will process your message asynchronously.", to_agent_name, new_run_id),
          false,
        ));
      }

      // Wait mode: poll run state until terminal
      let timeout = tokio::time::Duration::from_secs(120);
      let start = tokio::time::Instant::now();
      loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        if start.elapsed() > timeout {
          return Ok((
            format!("Timed out waiting for agent '{}' (run {}). The agent may still be running.", to_agent_name, new_run_id),
            false,
          ));
        }

        let pool = db.clone();
        let rid = new_run_id.clone();
        let state: Option<String> = tokio::task::spawn_blocking(move || {
          let conn = pool.get().ok()?;
          conn.query_row("SELECT state FROM runs WHERE id = ?1", rusqlite::params![rid], |row| row.get(0)).ok()
        })
        .await
        .ok()
        .flatten();

        match state.as_deref() {
          Some("success") | Some("failure") | Some("cancelled") | Some("timed_out") => {
            let terminal_state = state.unwrap();
            // Try to get the finish summary from the run's metadata or logs
            let pool = db.clone();
            let rid = new_run_id.clone();
            let summary = tokio::task::spawn_blocking(move || -> Option<String> {
              let conn = pool.get().ok()?;
              let meta: String = conn.query_row(
                "SELECT metadata FROM runs WHERE id = ?1",
                rusqlite::params![rid],
                |row| row.get(0),
              ).ok()?;
              let meta_val: serde_json::Value = serde_json::from_str(&meta).ok()?;
              meta_val.get("finish_summary").and_then(|s| s.as_str()).map(|s| s.to_string())
            })
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| format!("Agent '{}' finished with state: {}", to_agent_name, terminal_state));

            return Ok((summary, false));
          }
          _ => continue,
        }
      }
    }

    "spawn_sub_agents" => {
      // Belt-and-suspenders: reject if this is already a sub-agent
      if ctx.is_sub_agent {
        return Ok(("Error: Sub-agents cannot spawn further sub-agents.".to_string(), false));
      }

      let tasks = input["tasks"].as_array().ok_or("spawn_sub_agents: missing 'tasks' array")?;
      if tasks.is_empty() {
        return Ok(("Error: 'tasks' array must not be empty.".to_string(), false));
      }
      if tasks.len() > MAX_SUB_AGENTS {
        return Ok((
          format!("Error: Maximum {} sub-agents allowed, got {}.", MAX_SUB_AGENTS, tasks.len()),
          false,
        ));
      }

      let timeout_secs = input["timeout_seconds"]
        .as_u64()
        .unwrap_or(DEFAULT_SUB_AGENT_TIMEOUT_SECS)
        .min(MAX_SUB_AGENT_TIMEOUT_SECS);

      let db = ctx.db.as_ref().ok_or("spawn_sub_agents: database not available")?;
      let bus_app = ctx.app.as_ref().ok_or("spawn_sub_agents: app handle not available")?;
      let executor_tx = ctx.executor_tx.as_ref().ok_or("spawn_sub_agents: executor channel not available")?;
      let parent_run_id = ctx.current_run_id.as_deref().unwrap_or("unknown");
      let agent_id = ctx.current_agent_id.as_deref().unwrap_or(&ctx.agent_id);
      let next_depth = ctx.chain_depth + 1;

      info!(run_id = run_id, count = tasks.len(), "agent tool: spawn_sub_agents");

      // Parse and validate sub-tasks
      struct SubTask {
        id: String,
        goal: String,
        run_id: String,
        task_id: String,
        log_path: PathBuf,
      }

      let mut sub_tasks: Vec<SubTask> = Vec::new();
      for item in tasks {
        let id = item["id"].as_str().ok_or("spawn_sub_agents: each task needs an 'id' field")?;
        let goal = item["goal"].as_str().ok_or("spawn_sub_agents: each task needs a 'goal' field")?;
        let sub_run_id = ulid::Ulid::new().to_string();
        let sub_task_id = ulid::Ulid::new().to_string();
        let log_path = PathBuf::from(format!(
          "{}/.orbit/logs/{}.log",
          std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
          sub_run_id
        ));

        sub_tasks.push(SubTask {
          id: id.to_string(),
          goal: goal.to_string(),
          run_id: sub_run_id,
          task_id: sub_task_id,
          log_path,
        });
      }

      // Insert ephemeral task and run records for each sub-agent
      let now = chrono::Utc::now().to_rfc3339();
      for st in &sub_tasks {
        let loop_config = AgentLoopConfig {
          goal: st.goal.clone(),
          model: None,
          max_iterations: None,
          max_total_tokens: None,
          template_vars: None,
        };
        let config_json = serde_json::to_string(&serde_json::to_value(&loop_config).map_err(|e| e.to_string())?)
          .map_err(|e| e.to_string())?;

        let pool = db.clone();
        let task_id = st.task_id.clone();
        let sub_run_id = st.run_id.clone();
        let agent_id = agent_id.to_string();
        let parent_run_id = parent_run_id.to_string();
        let log_path = st.log_path.to_string_lossy().to_string();
        let now = now.clone();
        let st_id = st.id.clone();

        tokio::task::spawn_blocking(move || {
          let conn = pool.get().map_err(|e| e.to_string())?;
          conn.execute(
            "INSERT INTO tasks (id, name, kind, config, max_duration_seconds, max_retries, retry_delay_seconds, concurrency_policy, tags, agent_id, session_id, enabled, created_at, updated_at)
             VALUES (?1, ?2, 'agent_loop', ?3, ?4, 0, 0, 'allow', '[]', ?5, NULL, 1, ?6, ?6)",
            rusqlite::params![task_id, format!("sub-agent:{}", st_id), config_json, timeout_secs as i64, agent_id, now],
          ).map_err(|e| e.to_string())?;
          conn.execute(
            "INSERT INTO runs (id, task_id, schedule_id, agent_id, state, trigger, log_path, retry_count, parent_run_id, metadata, chain_depth, is_sub_agent, created_at)
             VALUES (?1, ?2, NULL, ?3, 'pending', 'sub_agent', ?4, 0, ?5, ?6, ?7, 1, ?8)",
            rusqlite::params![sub_run_id, task_id, agent_id, log_path, parent_run_id, json!({"sub_task_id": st_id}).to_string(), next_depth, now],
          ).map_err(|e| e.to_string())?;
          Ok::<(), String>(())
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
      }

      // Emit event so UI can track sub-agents
      let sub_run_ids: Vec<String> = sub_tasks.iter().map(|s| s.run_id.clone()).collect();
      emit_sub_agents_spawned(bus_app, parent_run_id, sub_run_ids);

      // Enqueue all sub-agents to the executor engine
      for st in &sub_tasks {
        let loop_config = AgentLoopConfig {
          goal: st.goal.clone(),
          model: None,
          max_iterations: None,
          max_total_tokens: None,
          template_vars: None,
        };
        let config_json = serde_json::to_value(&loop_config).map_err(|e| e.to_string())?;

        let task = Task {
          id: st.task_id.clone(),
          name: format!("sub-agent:{}", st.id),
          description: Some(format!("Sub-agent task '{}'", st.id)),
          kind: "agent_loop".to_string(),
          config: config_json,
          max_duration_seconds: timeout_secs as i64,
          max_retries: 0,
          retry_delay_seconds: 0,
          concurrency_policy: "allow".to_string(),
          tags: vec!["sub_agent".to_string()],
          agent_id: Some(agent_id.to_string()),
          session_id: None,
          enabled: true,
          created_at: now.clone(),
          updated_at: now.clone(),
        };

        let req = RunRequest {
          run_id: st.run_id.clone(),
          task,
          schedule_id: None,
          _trigger: "sub_agent".to_string(),
          retry_count: 0,
          _parent_run_id: Some(parent_run_id.to_string()),
          chain_depth: next_depth,
        };
        executor_tx.send(req).map_err(|e| format!("failed to enqueue sub-agent: {}", e))?;
      }

      // Poll all sub-agent runs until they reach terminal state
      let timeout = tokio::time::Duration::from_secs(timeout_secs + 30); // extra grace period
      let start = tokio::time::Instant::now();
      let sub_run_ids: Vec<(String, String)> = sub_tasks.iter().map(|s| (s.id.clone(), s.run_id.clone())).collect();

      loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        let all_done = {
          let pool = db.clone();
          let ids: Vec<String> = sub_run_ids.iter().map(|(_, rid)| rid.clone()).collect();
          tokio::task::spawn_blocking(move || -> bool {
            let conn = match pool.get() {
              Ok(c) => c,
              Err(_) => return false,
            };
            for rid in &ids {
              let state: Option<String> = conn.query_row(
                "SELECT state FROM runs WHERE id = ?1",
                rusqlite::params![rid],
                |row| row.get(0),
              ).ok();
              match state.as_deref() {
                Some("success") | Some("failure") | Some("cancelled") | Some("timed_out") => {}
                _ => return false,
              }
            }
            true
          })
          .await
          .unwrap_or(false)
        };

        if all_done {
          break;
        }

        if start.elapsed() > timeout {
          warn!(run_id = run_id, "spawn_sub_agents: timed out waiting for sub-agents");
          break;
        }
      }

      // Collect results from all sub-agents
      let mut results = Vec::new();
      for (task_id, sub_run_id) in &sub_run_ids {
        let pool = db.clone();
        let rid = sub_run_id.clone();
        let result = tokio::task::spawn_blocking(move || -> (String, Option<String>) {
          let conn = match pool.get() {
            Ok(c) => c,
            Err(_) => return ("failure".to_string(), None),
          };
          let state: String = conn.query_row(
            "SELECT state FROM runs WHERE id = ?1",
            rusqlite::params![rid],
            |row| row.get(0),
          ).unwrap_or_else(|_| "failure".to_string());

          let meta: Option<String> = conn.query_row(
            "SELECT metadata FROM runs WHERE id = ?1",
            rusqlite::params![rid],
            |row| row.get(0),
          ).ok();
          let summary = meta.and_then(|m| {
            let val: serde_json::Value = serde_json::from_str(&m).ok()?;
            val.get("finish_summary").and_then(|s| s.as_str()).map(|s| s.to_string())
          });

          (state, summary)
        })
        .await
        .unwrap_or(("failure".to_string(), None));

        let (state, summary) = result;
        match state.as_str() {
          "success" => {
            results.push(json!({
              "id": task_id,
              "status": "success",
              "summary": summary.unwrap_or_else(|| "Sub-agent completed successfully.".to_string()),
            }));
          }
          "timed_out" => {
            results.push(json!({
              "id": task_id,
              "status": "timed_out",
              "error": format!("Sub-agent timed out after {}s.", timeout_secs),
            }));
          }
          _ => {
            results.push(json!({
              "id": task_id,
              "status": state,
              "error": summary.unwrap_or_else(|| format!("Sub-agent finished with state: {}", state)),
            }));
          }
        }
      }

      let response = json!({ "results": results });
      Ok((serde_json::to_string_pretty(&response).unwrap_or_else(|_| response.to_string()), false))
    }

    "activate_skill" => {
      let skill_name = input["skill_name"].as_str().ok_or("activate_skill: missing 'skill_name' field")?;

      info!(run_id = run_id, skill = skill_name, "agent tool: activate_skill");

      let instructions = skills::load_skill_instructions(
        &ctx.agent_id,
        skill_name,
        &ctx.disabled_skills,
      )?;

      Ok((
        format!("<skill-instructions name=\"{}\">\n{}\n</skill-instructions>", skill_name, instructions),
        false,
      ))
    }

    "finish" => {
      let summary = input["summary"].as_str().unwrap_or("Agent finished without summary");
      Ok((summary.to_string(), true))
    }

    other => Err(format!("unknown tool: {}", other)),
  }
}

// ─── Web search providers ───────────────────────────────────────────────────

async fn execute_web_search(provider: &str, query: &str, count: u32) -> Result<String, String> {
  match provider {
    "brave" => brave_search(query, count).await,
    "tavily" => tavily_search(query, count).await,
    other => Err(format!("unsupported search provider: {}", other)),
  }
}

async fn brave_search(query: &str, count: u32) -> Result<String, String> {
  let api_key = super::keychain::retrieve_api_key("brave")
    .map_err(|_| "No API key for Brave Search. Set it in the Agent Config tab (provider: brave).".to_string())?;

  let client = reqwest::Client::new();
  let resp = client
    .get("https://api.search.brave.com/res/v1/web/search")
    .header("X-Subscription-Token", &api_key)
    .header("Accept", "application/json")
    .query(&[("q", query), ("count", &count.to_string())])
    .send()
    .await
    .map_err(|e| format!("Brave search request failed: {}", e))?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    return Err(format!("Brave search returned {}: {}", status, body));
  }

  let json: serde_json::Value = resp
    .json()
    .await
    .map_err(|e| format!("Failed to parse Brave search response: {}", e))?;

  let mut results = Vec::new();

  if let Some(web_results) = json["web"].get("results").and_then(|r| r.as_array()) {
    for (i, item) in web_results.iter().enumerate() {
      let title = item["title"].as_str().unwrap_or("(no title)");
      let url = item["url"].as_str().unwrap_or("");
      let description = item["description"].as_str().unwrap_or("(no description)");
      results.push(format!("{}. {}\n   {}\n   {}", i + 1, title, url, description));
    }
  }

  if results.is_empty() {
    Ok("No results found.".to_string())
  } else {
    Ok(results.join("\n\n"))
  }
}

async fn tavily_search(query: &str, count: u32) -> Result<String, String> {
  let api_key = super::keychain::retrieve_api_key("tavily")
    .map_err(|_| "No API key for Tavily. Set it in the Agent Config tab (provider: tavily).".to_string())?;

  let client = reqwest::Client::new();
  let body = json!({
    "query": query,
    "max_results": count,
    "search_depth": "basic"
  });

  let resp = client
    .post("https://api.tavily.com/search")
    .header("Authorization", format!("Bearer {}", api_key))
    .json(&body)
    .send()
    .await
    .map_err(|e| format!("Tavily search request failed: {}", e))?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    return Err(format!("Tavily search returned {}: {}", status, body));
  }

  let json: serde_json::Value = resp
    .json()
    .await
    .map_err(|e| format!("Failed to parse Tavily search response: {}", e))?;

  let mut results = Vec::new();

  if let Some(items) = json["results"].as_array() {
    for (i, item) in items.iter().enumerate() {
      let title = item["title"].as_str().unwrap_or("(no title)");
      let url = item["url"].as_str().unwrap_or("");
      let content = item["content"].as_str().unwrap_or("(no content)");
      results.push(format!("{}. {}\n   {}\n   {}", i + 1, title, url, content));
    }
  }

  if results.is_empty() {
    Ok("No results found.".to_string())
  } else {
    Ok(results.join("\n\n"))
  }
}
