use serde_json::json;
use tracing::{info, warn};

use crate::events::emitter::emit_sub_agents_spawned;
use crate::executor::llm_provider::ToolDefinition;
use crate::executor::permissions::PermissionRegistry;
use crate::executor::session_agent;

use super::{context::ToolExecutionContext, ToolHandler};

/// Maximum number of sub-agents per spawn call.
const MAX_SUB_AGENTS: usize = 10;
/// Default timeout for sub-agent execution in seconds.
const DEFAULT_SUB_AGENT_TIMEOUT_SECS: u64 = 300;
/// Maximum timeout for sub-agent execution in seconds.
const MAX_SUB_AGENT_TIMEOUT_SECS: u64 = 600;

pub struct SpawnSubAgentsTool;

#[async_trait::async_trait]
impl ToolHandler for SpawnSubAgentsTool {
    fn name(&self) -> &'static str {
        "spawn_sub_agents"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
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
        }
    }

    async fn execute(
        &self,
        ctx: &ToolExecutionContext,
        input: &serde_json::Value,
        _app: &tauri::AppHandle,
        run_id: &str,
    ) -> Result<(String, bool), String> {
        if !ctx.allow_sub_agents {
            return Ok((
                "Error: Sub-agents cannot spawn further sub-agents.".to_string(),
                false,
            ));
        }

        let tasks = input["tasks"]
            .as_array()
            .ok_or("spawn_sub_agents: missing 'tasks' array")?;
        if tasks.is_empty() {
            return Ok(("Error: 'tasks' array must not be empty.".to_string(), false));
        }
        if tasks.len() > MAX_SUB_AGENTS {
            return Ok((
                format!(
                    "Error: Maximum {} sub-agents allowed, got {}.",
                    MAX_SUB_AGENTS,
                    tasks.len()
                ),
                false,
            ));
        }

        let timeout_secs = input["timeout_seconds"]
            .as_u64()
            .unwrap_or(DEFAULT_SUB_AGENT_TIMEOUT_SECS)
            .min(MAX_SUB_AGENT_TIMEOUT_SECS);

        let db = ctx
            .db
            .as_ref()
            .ok_or("spawn_sub_agents: database not available")?;
        let bus_app = ctx
            .app
            .as_ref()
            .ok_or("spawn_sub_agents: app handle not available")?;
        let executor_tx = ctx
            .executor_tx
            .as_ref()
            .ok_or("spawn_sub_agents: executor channel not available")?;
        let agent_semaphores = ctx
            .agent_semaphores
            .as_ref()
            .ok_or("spawn_sub_agents: agent semaphores not available")?;
        let session_registry = ctx
            .session_registry
            .as_ref()
            .ok_or("spawn_sub_agents: session registry not available")?;
        let agent_id = ctx.current_agent_id.as_deref().unwrap_or(&ctx.agent_id);
        let parent_session_id = ctx.current_session_id.clone();
        let next_depth = ctx.chain_depth + 1;

        info!(
            run_id = run_id,
            count = tasks.len(),
            "agent tool: spawn_sub_agents"
        );

        struct SubTask {
            id: String,
            goal: String,
            session_id: String,
        }

        let mut sub_tasks: Vec<SubTask> = Vec::new();
        for item in tasks {
            let id = item["id"]
                .as_str()
                .ok_or("spawn_sub_agents: each task needs an 'id' field")?;
            let goal = item["goal"]
                .as_str()
                .ok_or("spawn_sub_agents: each task needs a 'goal' field")?;
            sub_tasks.push(SubTask {
                id: id.to_string(),
                goal: goal.to_string(),
                session_id: ulid::Ulid::new().to_string(),
            });
        }

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
                       chain_depth, execution_state, finish_summary, terminal_error, created_at, updated_at,
                       allow_sub_agents
                     ) VALUES (?1, ?2, ?3, 0, 'sub_agent', ?4, NULL, ?5, 'queued', NULL, NULL, ?6, ?6, 0)",
                    rusqlite::params![session_id, agent_id, title, parent_session_id, next_depth, now],
                )
                .map_err(|e| e.to_string())?;

                let user_content = serde_json::to_string(&vec![serde_json::json!({
                    "type": "text",
                    "text": goal,
                })])
                .map_err(|e| e.to_string())?;
                conn.execute(
                    "INSERT INTO chat_messages (id, session_id, role, content, created_at)
                     VALUES (?1, ?2, 'user', ?3, ?4)",
                    rusqlite::params![ulid::Ulid::new().to_string(), session_id, user_content, now],
                )
                .map_err(|e| e.to_string())?;
                Ok::<(), String>(())
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        }

        let sub_session_ids: Vec<String> = sub_tasks.iter().map(|s| s.session_id.clone()).collect();
        emit_sub_agents_spawned(
            bus_app,
            parent_session_id.as_deref(),
            ctx.current_run_id.as_deref(),
            sub_session_ids.clone(),
        );

        for st in &sub_tasks {
            let db_clone = db.clone();
            let app_clone = bus_app.clone();
            let tx_clone = executor_tx.clone();
            let semaphores = agent_semaphores.clone();
            let registry = session_registry.clone();
            let perm_registry = ctx
                .permission_registry
                .clone()
                .unwrap_or_else(PermissionRegistry::new);
            let question_registry = ctx.user_question_registry.clone();
            let mem_client = ctx.memory_client.clone();
            let mem_user_id = ctx.memory_user_id.clone();
            let cloud_cl = ctx.cloud_client.clone();
            let sub_agent_id = agent_id.to_string();
            let sub_session_id = st.session_id.clone();
            tokio::task::spawn_blocking(move || {
                tauri::async_runtime::block_on(async move {
                    if let Err(e) = session_agent::run_agent_session(
                        &sub_agent_id,
                        &sub_session_id,
                        next_depth,
                        true,
                        false,
                        &db_clone,
                        &app_clone,
                        &tx_clone,
                        &semaphores,
                        &registry,
                        &perm_registry,
                        question_registry.as_ref(),
                        mem_client.as_ref(),
                        &mem_user_id,
                        cloud_cl,
                    )
                    .await
                    {
                        warn!(session_id = %sub_session_id, "sub-agent session failed: {}", e);
                    }
                })
            });
        }

        let timeout = tokio::time::Duration::from_secs(timeout_secs + 30);
        let start = tokio::time::Instant::now();
        let sub_session_refs: Vec<(String, String)> = sub_tasks
            .iter()
            .map(|s| (s.id.clone(), s.session_id.clone()))
            .collect();

        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            let all_done = {
                let pool = db.clone();
                let ids: Vec<String> = sub_session_refs
                    .iter()
                    .map(|(_, sid)| sid.clone())
                    .collect();
                tokio::task::spawn_blocking(move || -> bool {
                    let conn = match pool.get() {
                        Ok(c) => c,
                        Err(_) => return false,
                    };
                    for sid in &ids {
                        let state: Option<String> = conn
                            .query_row(
                                "SELECT execution_state FROM chat_sessions WHERE id = ?1",
                                rusqlite::params![sid],
                                |row| row.get(0),
                            )
                            .ok();
                        match state.as_deref() {
                            Some("success") | Some("failure") | Some("cancelled")
                            | Some("timed_out") => {}
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
                warn!(
                    run_id = run_id,
                    "spawn_sub_agents: timed out waiting for sub-agents"
                );
                for (_, session_id) in &sub_session_refs {
                    session_registry.cancel(session_id).await;
                    let _ = session_agent::update_session_execution_state(
                        db,
                        session_id,
                        "timed_out",
                        None,
                        Some(format!("Sub-agent timed out after {}s.", timeout_secs)),
                    )
                    .await;
                }
                break;
            }
        }

        let mut results = Vec::new();
        for (task_id, sub_session_id) in &sub_session_refs {
            let pool = db.clone();
            let sid = sub_session_id.clone();
            let result = tokio::task::spawn_blocking(move || -> (String, Option<String>, Option<String>) {
                let conn = match pool.get() {
                    Ok(c) => c,
                    Err(_) => {
                        return (
                            "failure".to_string(),
                            None,
                            Some("Database unavailable".to_string()),
                        )
                    }
                };
                conn.query_row(
                    "SELECT COALESCE(execution_state, 'failure'), finish_summary, terminal_error FROM chat_sessions WHERE id = ?1",
                    rusqlite::params![sid],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap_or_else(|_| ("failure".to_string(), None, Some("Session not found".to_string())))
            })
            .await
            .unwrap_or((
                "failure".to_string(),
                None,
                Some("Join error".to_string()),
            ));

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
        Ok((
            serde_json::to_string_pretty(&response).unwrap_or_else(|_| response.to_string()),
            false,
        ))
    }
}
