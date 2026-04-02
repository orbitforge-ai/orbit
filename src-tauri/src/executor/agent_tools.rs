use serde_json::json;
use std::path::{ Path, PathBuf };
use tokio::sync::mpsc;
use tracing::{ info, warn };

use crate::db::DbPool;
use crate::events::emitter::{ emit_bus_message_sent, emit_log_chunk, emit_sub_agents_spawned };
use crate::executor::engine::{ AgentSemaphores, RunRequest, SessionExecutionRegistry };
use crate::executor::llm_provider::ToolDefinition;
use crate::executor::memory::MemoryClient;
use crate::executor::permissions::PermissionRegistry;
use crate::executor::session_agent;
use crate::executor::skills;

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
  pub current_session_id: Option<String>,
  pub chain_depth: i64,
  pub agent_semaphores: Option<AgentSemaphores>,
  pub session_registry: Option<SessionExecutionRegistry>,
  /// Whether this context is for a sub-agent (prevents nesting).
  pub is_sub_agent: bool,
  /// Permission registry for gating tool execution.
  pub permission_registry: Option<PermissionRegistry>,
  /// Optional memory client for long-term memory operations.
  pub memory_client: Option<MemoryClient>,
  /// User ID used for scoping memory operations (Supabase user_id when cloud, else "default_user").
  pub memory_user_id: String,
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
      current_session_id: None,
      chain_depth: 0,
      agent_semaphores: None,
      session_registry: None,
      is_sub_agent: false,
      permission_registry: None,
      memory_client: None,
      memory_user_id: "default_user".to_string(),
    }
  }

  pub fn new_with_bus(
    agent_id: &str,
    run_id: &str,
    session_id: Option<&str>,
    chain_depth: i64,
    db: DbPool,
    executor_tx: mpsc::UnboundedSender<RunRequest>,
    app: tauri::AppHandle,
    agent_semaphores: AgentSemaphores,
    session_registry: SessionExecutionRegistry,
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
      current_session_id: session_id.map(|s| s.to_string()),
      chain_depth,
      agent_semaphores: Some(agent_semaphores),
      session_registry: Some(session_registry),
      is_sub_agent: false,
      permission_registry: None,
      memory_client: None,
      memory_user_id: "default_user".to_string(),
    }
  }

  /// Set the permission registry on this context (builder pattern).
  pub fn with_permission_registry(mut self, registry: PermissionRegistry) -> Self {
    self.permission_registry = Some(registry);
    self
  }

  /// Set the memory client on this context (builder pattern).
  pub fn with_memory_client(mut self, client: Option<MemoryClient>) -> Self {
    self.memory_client = client;
    self
  }

  /// Set the user ID for memory scoping (builder pattern).
  pub fn with_memory_user_id(mut self, user_id: String) -> Self {
    self.memory_user_id = user_id;
    self
  }

  pub fn new_for_sub_agent(
    agent_id: &str,
    run_id: &str,
    session_id: Option<&str>,
    chain_depth: i64,
    db: DbPool,
    executor_tx: mpsc::UnboundedSender<RunRequest>,
    app: tauri::AppHandle,
    agent_semaphores: AgentSemaphores,
    session_registry: SessionExecutionRegistry,
  ) -> Self {
    let mut ctx = Self::new_with_bus(
      agent_id,
      run_id,
      session_id,
      chain_depth,
      db,
      executor_tx,
      app,
      agent_semaphores,
      session_registry,
    );
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
      name: "remember".to_string(),
      description: "Save a piece of information to long-term memory. Use this to persist important facts, user preferences, feedback, or project context across sessions.".to_string(),
      input_schema: json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The information to remember"
                    },
                    "memory_type": {
                        "type": "string",
                        "enum": ["user", "feedback", "project", "reference"],
                        "description": "Category: 'user' for user facts/preferences, 'feedback' for guidance on your approach, 'project' for project context/decisions, 'reference' for pointers to external resources"
                    }
                },
                "required": ["text", "memory_type"]
            }),
    },
    ToolDefinition {
      name: "forget".to_string(),
      description: "Remove a memory by searching for the best match and deleting it.".to_string(),
      input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Description of the memory to forget"
                    }
                },
                "required": ["query"]
            }),
    },
    ToolDefinition {
      name: "search_memory".to_string(),
      description: "Search long-term memory for relevant information using semantic similarity.".to_string(),
      input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "What to search for"
                    },
                    "memory_type": {
                        "type": "string",
                        "enum": ["user", "feedback", "project", "reference"],
                        "description": "Optional: filter by memory category"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum results to return (default: 5, max: 20)"
                    }
                },
                "required": ["query"]
            }),
    },
    ToolDefinition {
      name: "list_memories".to_string(),
      description: "List all memories, optionally filtered by type.".to_string(),
      input_schema: json!({
                "type": "object",
                "properties": {
                    "memory_type": {
                        "type": "string",
                        "enum": ["user", "feedback", "project", "reference"],
                        "description": "Optional: filter by memory category"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum results to return (default: 50, max: 200)"
                    }
                }
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
      let bus_app = ctx.app.as_ref().ok_or("send_message: app handle not available")?;
      let agent_semaphores = ctx.agent_semaphores.as_ref().ok_or("send_message: agent semaphores not available")?;
      let session_registry = ctx.session_registry.as_ref().ok_or("send_message: session registry not available")?;
      let from_agent = ctx.current_agent_id.as_deref().unwrap_or("unknown");
      let from_run: Option<String> = ctx.current_run_id.as_ref().and_then(|rid| {
        if rid.starts_with("chat:") { None } else { Some(rid.clone()) }
      });
      let from_session = ctx.current_session_id.clone();

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
      let new_session_id = ulid::Ulid::new().to_string();
      let now = chrono::Utc::now().to_rfc3339();
      {
        let pool = db.clone();
        let msg_id = msg_id.clone();
        let new_session_id = new_session_id.clone();
        let from_agent = from_agent.to_string();
        let from_run = from_run.clone();
        let from_session = from_session.clone();
        let to_agent_id = to_agent_id.clone();
        let payload_str = payload_str.to_string();
        let now = now.clone();
        let title = payload_str.chars().take(60).collect::<String>();

        tokio::task::spawn_blocking(move || {
          let conn = pool.get().map_err(|e| e.to_string())?;
          conn.execute(
            "INSERT INTO chat_sessions (
               id, agent_id, title, archived, session_type, parent_session_id, source_bus_message_id,
               chain_depth, execution_state, finish_summary, terminal_error, created_at, updated_at
             ) VALUES (?1, ?2, ?3, 0, 'bus_message', NULL, NULL, ?4, 'queued', NULL, NULL, ?5, ?5)",
            rusqlite::params![new_session_id, to_agent_id, title, next_depth, now],
          ).map_err(|e| e.to_string())?;

          // Wrap bus message payload in data tags to prevent prompt injection
          let wrapped_payload = format!(
            "<agent_message from=\"{}\" untrusted=\"true\">{}</agent_message>",
            from_agent, payload_str
          );
          let user_content = serde_json::to_string(&vec![serde_json::json!({
            "type": "text",
            "text": wrapped_payload,
          })]).map_err(|e| e.to_string())?;
          conn.execute(
            "INSERT INTO chat_messages (id, session_id, role, content, created_at)
             VALUES (?1, ?2, 'user', ?3, ?4)",
            rusqlite::params![ulid::Ulid::new().to_string(), new_session_id, user_content, now],
          ).map_err(|e| e.to_string())?;

          conn.execute(
            "INSERT INTO bus_messages (
               id, from_agent_id, from_run_id, from_session_id, to_agent_id, to_run_id, to_session_id,
               kind, payload, status, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, 'direct', ?7, 'delivered', ?8)",
            rusqlite::params![msg_id, from_agent, from_run, from_session, to_agent_id, new_session_id, payload_str, now],
          ).map_err(|e| e.to_string())?;

          conn.execute(
            "UPDATE chat_sessions SET source_bus_message_id = ?1 WHERE id = ?2",
            rusqlite::params![msg_id, new_session_id],
          ).map_err(|e| e.to_string())?;

          Ok::<(), String>(())
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
      }

      let db_clone = db.clone();
      let app_clone = bus_app.clone();
      let tx_clone = ctx.executor_tx.as_ref().ok_or("send_message: executor channel not available")?.clone();
      let semaphores = agent_semaphores.clone();
      let registry = session_registry.clone();
      let perm_registry = ctx.permission_registry.clone().unwrap_or_else(PermissionRegistry::new);
      let mem_client = ctx.memory_client.clone();
      let mem_user_id = ctx.memory_user_id.clone();
      let target_agent_id = to_agent_id.clone();
      let target_session_id = new_session_id.clone();
      tokio::task::spawn_blocking(move || {
        tauri::async_runtime::block_on(async move {
          if let Err(e) = session_agent::run_agent_session(
            &target_agent_id,
            &target_session_id,
            next_depth,
            false,
            &db_clone,
            &app_clone,
            &tx_clone,
            &semaphores,
            &registry,
            &perm_registry,
            mem_client.as_ref(),
            &mem_user_id,
          ).await {
            warn!(session_id = %target_session_id, "send_message session failed: {}", e);
          }
        })
      });

      // Emit bus event
      emit_bus_message_sent(
        bus_app,
        &msg_id,
        from_agent,
        &to_agent_id,
        "direct",
        json!({ "message": payload_str }),
        Some(&new_session_id),
        None,
      );

      if !wait {
        return Ok((
          format!("Message sent to agent '{}'. Session ID: {}. The agent will process your message asynchronously.", to_agent_name, new_session_id),
          false,
        ));
      }

      // Wait mode: poll session state until terminal
      let timeout = tokio::time::Duration::from_secs(120);
      let start = tokio::time::Instant::now();
      loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        if start.elapsed() > timeout {
          return Ok((
            format!("Timed out waiting for agent '{}' (session {}). The agent may still be running.", to_agent_name, new_session_id),
            false,
          ));
        }

        let pool = db.clone();
        let sid = new_session_id.clone();
        let result: Option<(Option<String>, Option<String>, Option<String>)> = tokio::task::spawn_blocking(move || {
          let conn = pool.get().ok()?;
          conn.query_row(
            "SELECT execution_state, finish_summary, terminal_error FROM chat_sessions WHERE id = ?1",
            rusqlite::params![sid],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
          ).ok()
        })
        .await
        .ok()
        .flatten();

        match result {
          Some((Some(state), finish_summary, terminal_error))
            if matches!(state.as_str(), "success" | "failure" | "cancelled" | "timed_out") =>
          {
            let summary = finish_summary
              .or(terminal_error)
              .unwrap_or_else(|| format!("Agent '{}' finished with state: {}", to_agent_name, state));
            return Ok((summary, false));
          }
          _ => {}
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
      let agent_semaphores = ctx.agent_semaphores.as_ref().ok_or("spawn_sub_agents: agent semaphores not available")?;
      let session_registry = ctx.session_registry.as_ref().ok_or("spawn_sub_agents: session registry not available")?;
      let agent_id = ctx.current_agent_id.as_deref().unwrap_or(&ctx.agent_id);
      let parent_session_id = ctx.current_session_id.clone();
      let next_depth = ctx.chain_depth + 1;

      info!(run_id = run_id, count = tasks.len(), "agent tool: spawn_sub_agents");

      // Parse and validate sub-tasks
      struct SubTask {
        id: String,
        goal: String,
        session_id: String,
      }

      let mut sub_tasks: Vec<SubTask> = Vec::new();
      for item in tasks {
        let id = item["id"].as_str().ok_or("spawn_sub_agents: each task needs an 'id' field")?;
        let goal = item["goal"].as_str().ok_or("spawn_sub_agents: each task needs a 'goal' field")?;
        sub_tasks.push(SubTask {
          id: id.to_string(),
          goal: goal.to_string(),
          session_id: ulid::Ulid::new().to_string(),
        });
      }

      // Create session records for each sub-agent
      let now = chrono::Utc::now().to_rfc3339();
      for st in &sub_tasks {
        let pool = db.clone();
        let session_id = st.session_id.clone();
        let title = st.id.clone();
        let goal = st.goal.clone();
        let agent_id = agent_id.to_string();
        let parent_session_id = parent_session_id.clone();
        let now = now.clone();

        tokio::task::spawn_blocking(move || {
          let conn = pool.get().map_err(|e| e.to_string())?;
          conn.execute(
            "INSERT INTO chat_sessions (
               id, agent_id, title, archived, session_type, parent_session_id, source_bus_message_id,
               chain_depth, execution_state, finish_summary, terminal_error, created_at, updated_at
             ) VALUES (?1, ?2, ?3, 0, 'sub_agent', ?4, NULL, ?5, 'queued', NULL, NULL, ?6, ?6)",
            rusqlite::params![session_id, agent_id, title, parent_session_id, next_depth, now],
          ).map_err(|e| e.to_string())?;

          let user_content = serde_json::to_string(&vec![serde_json::json!({
            "type": "text",
            "text": goal,
          })]).map_err(|e| e.to_string())?;
          conn.execute(
            "INSERT INTO chat_messages (id, session_id, role, content, created_at)
             VALUES (?1, ?2, 'user', ?3, ?4)",
            rusqlite::params![ulid::Ulid::new().to_string(), session_id, user_content, now],
          ).map_err(|e| e.to_string())?;
          Ok::<(), String>(())
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
      }

      // Emit event so UI can track sub-agents
      let sub_session_ids: Vec<String> = sub_tasks.iter().map(|s| s.session_id.clone()).collect();
      emit_sub_agents_spawned(
        bus_app,
        parent_session_id.as_deref(),
        ctx.current_run_id.as_deref(),
        sub_session_ids.clone(),
      );

      // Spawn all sub-agents
      for st in &sub_tasks {
        let db_clone = db.clone();
        let app_clone = bus_app.clone();
        let tx_clone = executor_tx.clone();
        let semaphores = agent_semaphores.clone();
        let registry = session_registry.clone();
        let perm_registry = ctx.permission_registry.clone().unwrap_or_else(PermissionRegistry::new);
        let mem_client = ctx.memory_client.clone();
        let mem_user_id = ctx.memory_user_id.clone();
        let sub_agent_id = agent_id.to_string();
        let sub_session_id = st.session_id.clone();
        tokio::task::spawn_blocking(move || {
          tauri::async_runtime::block_on(async move {
            if let Err(e) = session_agent::run_agent_session(
              &sub_agent_id,
              &sub_session_id,
              next_depth,
              true,
              &db_clone,
              &app_clone,
              &tx_clone,
              &semaphores,
              &registry,
              &perm_registry,
              mem_client.as_ref(),
              &mem_user_id,
            ).await {
              warn!(session_id = %sub_session_id, "sub-agent session failed: {}", e);
            }
          })
        });
      }

      // Poll all sub-agent sessions until they reach terminal state
      let timeout = tokio::time::Duration::from_secs(timeout_secs + 30); // extra grace period
      let start = tokio::time::Instant::now();
      let sub_session_refs: Vec<(String, String)> = sub_tasks.iter().map(|s| (s.id.clone(), s.session_id.clone())).collect();

      loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        let all_done = {
          let pool = db.clone();
          let ids: Vec<String> = sub_session_refs.iter().map(|(_, sid)| sid.clone()).collect();
          tokio::task::spawn_blocking(move || -> bool {
            let conn = match pool.get() {
              Ok(c) => c,
              Err(_) => return false,
            };
            for sid in &ids {
              let state: Option<String> = conn.query_row(
                "SELECT execution_state FROM chat_sessions WHERE id = ?1",
                rusqlite::params![sid],
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
          for (_, session_id) in &sub_session_refs {
            session_registry.cancel(session_id).await;
            let _ = session_agent::update_session_execution_state(
              db,
              session_id,
              "timed_out",
              None,
              Some(format!("Sub-agent timed out after {}s.", timeout_secs)),
            ).await;
          }
          break;
        }
      }

      // Collect results from all sub-agents
      let mut results = Vec::new();
      for (task_id, sub_session_id) in &sub_session_refs {
        let pool = db.clone();
        let sid = sub_session_id.clone();
        let result = tokio::task::spawn_blocking(move || -> (String, Option<String>, Option<String>) {
          let conn = match pool.get() {
            Ok(c) => c,
            Err(_) => return ("failure".to_string(), None, Some("Database unavailable".to_string())),
          };
          conn.query_row(
            "SELECT COALESCE(execution_state, 'failure'), finish_summary, terminal_error FROM chat_sessions WHERE id = ?1",
            rusqlite::params![sid],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
          ).unwrap_or_else(|_| ("failure".to_string(), None, Some("Session not found".to_string())))
        })
        .await
        .unwrap_or(("failure".to_string(), None, Some("Join error".to_string())));

        let (state, summary, terminal_error) = result;
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
              "error": terminal_error.unwrap_or_else(|| format!("Sub-agent timed out after {}s.", timeout_secs)),
            }));
          }
          _ => {
            results.push(json!({
              "id": task_id,
              "status": state,
              "error": terminal_error.or(summary).unwrap_or_else(|| format!("Sub-agent finished with state: {}", state)),
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

    "remember" => {
      let text = input["text"].as_str().ok_or("remember: missing 'text' field")?;
      let memory_type = input["memory_type"].as_str().ok_or("remember: missing 'memory_type' field")?;

      if !matches!(memory_type, "user" | "feedback" | "project" | "reference") {
        return Ok((
          format!("Error: invalid memory_type '{}'. Must be one of: user, feedback, project, reference", memory_type),
          false,
        ));
      }

      let client = match &ctx.memory_client {
        Some(c) => c,
        None => return Ok(("Memory service is not available.".to_string(), false)),
      };
      let agent_id = ctx.current_agent_id.as_deref().unwrap_or(&ctx.agent_id);

      info!(run_id = run_id, memory_type = memory_type, "agent tool: remember");

      match client.add_memory(text, memory_type, &ctx.memory_user_id, agent_id, None).await {
        Ok(_) => Ok((format!("Remembered: \"{}\" (type: {})", text, memory_type), false)),
        Err(e) => Ok((format!("Failed to save memory: {}", e), false)),
      }
    }

    "forget" => {
      let query = input["query"].as_str().ok_or("forget: missing 'query' field")?;

      let client = match &ctx.memory_client {
        Some(c) => c,
        None => return Ok(("Memory service is not available.".to_string(), false)),
      };
      let agent_id = ctx.current_agent_id.as_deref().unwrap_or(&ctx.agent_id);

      info!(run_id = run_id, query = query, "agent tool: forget");

      let matches = match client.search_memories(query, &ctx.memory_user_id, agent_id, None, 1).await {
        Ok(m) => m,
        Err(e) => return Ok((format!("Failed to search for memory to forget: {}", e), false)),
      };

      let Some(top) = matches.into_iter().next() else {
        return Ok(("No matching memory found.".to_string(), false));
      };

      let preview: String = top.text.chars().take(80).collect();
      match client.delete_memory(&top.id).await {
        Ok(()) => Ok((format!("Forgot: \"{}\"", preview), false)),
        Err(e) => Ok((format!("Failed to delete memory: {}", e), false)),
      }
    }

    "search_memory" => {
      let query = input["query"].as_str().ok_or("search_memory: missing 'query' field")?;
      let memory_type = input["memory_type"].as_str();
      let limit = input["limit"].as_u64().unwrap_or(5).min(20) as u32;

      let client = match &ctx.memory_client {
        Some(c) => c,
        None => return Ok(("Memory service is not available.".to_string(), false)),
      };
      let agent_id = ctx.current_agent_id.as_deref().unwrap_or(&ctx.agent_id);

      info!(run_id = run_id, query = query, "agent tool: search_memory");

      match client.search_memories(query, &ctx.memory_user_id, agent_id, memory_type, limit).await {
        Ok(entries) if entries.is_empty() => Ok(("No matching memories found.".to_string(), false)),
        Ok(entries) => {
          let lines: Vec<String> = entries
            .iter()
            .map(|e| format!("[{}] {} ({})", e.memory_type, e.text, e.created_at))
            .collect();
          Ok((lines.join("\n"), false))
        }
        Err(e) => Ok((format!("Memory search failed: {}", e), false)),
      }
    }

    "list_memories" => {
      let memory_type = input["memory_type"].as_str();
      let limit = input["limit"].as_u64().unwrap_or(50).min(200) as u32;

      let client = match &ctx.memory_client {
        Some(c) => c,
        None => return Ok(("Memory service is not available.".to_string(), false)),
      };
      let agent_id = ctx.current_agent_id.as_deref().unwrap_or(&ctx.agent_id);

      info!(run_id = run_id, "agent tool: list_memories");

      match client.list_memories(&ctx.memory_user_id, agent_id, memory_type, limit, 0).await {
        Ok(entries) if entries.is_empty() => Ok(("No memories stored.".to_string(), false)),
        Ok(entries) => {
          let lines: Vec<String> = entries
            .iter()
            .map(|e| format!("[{}] {} ({})", e.memory_type, e.text, e.created_at))
            .collect();
          Ok((lines.join("\n"), false))
        }
        Err(e) => Ok((format!("Failed to list memories: {}", e), false)),
      }
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
