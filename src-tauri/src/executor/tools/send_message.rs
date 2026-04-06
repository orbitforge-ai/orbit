use serde_json::json;
use tracing::{info, warn};

use crate::events::emitter::emit_bus_message_sent;
use crate::executor::llm_provider::ToolDefinition;
use crate::executor::permissions::PermissionRegistry;
use crate::executor::session_agent;

use super::{context::ToolExecutionContext, ToolHandler};

/// Maximum chain depth before agent bus rejects further sends.
const MAX_CHAIN_DEPTH: i64 = 10;

pub struct SendMessageTool;

#[async_trait::async_trait]
impl ToolHandler for SendMessageTool {
    fn name(&self) -> &'static str {
        "send_message"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
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
        }
    }

    async fn execute(
        &self,
        ctx: &ToolExecutionContext,
        input: &serde_json::Value,
        _app: &tauri::AppHandle,
        run_id: &str,
    ) -> Result<(String, bool), String> {
        let target = input["target_agent"]
            .as_str()
            .ok_or("send_message: missing 'target_agent' field")?;
        let message = input["message"]
            .as_str()
            .ok_or("send_message: missing 'message' field")?;
        let wait = input["wait_for_result"].as_bool().unwrap_or(false);

        info!(
            run_id = run_id,
            target = target,
            wait = wait,
            "agent tool: send_message"
        );

        let db = ctx
            .db
            .as_ref()
            .ok_or("send_message: agent bus not available in this context")?;
        let bus_app = ctx
            .app
            .as_ref()
            .ok_or("send_message: app handle not available")?;
        let agent_semaphores = ctx
            .agent_semaphores
            .as_ref()
            .ok_or("send_message: agent semaphores not available")?;
        let session_registry = ctx
            .session_registry
            .as_ref()
            .ok_or("send_message: session registry not available")?;
        let from_agent = ctx.current_agent_id.as_deref().unwrap_or("unknown");
        let from_run: Option<String> = ctx.current_run_id.as_ref().and_then(|rid| {
            if rid.starts_with("chat:") {
                None
            } else {
                Some(rid.clone())
            }
        });
        let from_session = ctx.current_session_id.clone();

        let next_depth = ctx.chain_depth + 1;
        if next_depth > MAX_CHAIN_DEPTH {
            return Ok((
                format!(
                    "Error: Maximum chain depth ({}) exceeded. Cannot trigger further agents to prevent infinite loops.",
                    MAX_CHAIN_DEPTH
                ),
                false,
            ));
        }

        let (to_agent_id, to_agent_name) = {
            let pool = db.clone();
            let target_str = target.to_string();
            tokio::task::spawn_blocking(move || {
                let conn = pool.get().map_err(|e| e.to_string())?;
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
                )
                .map_err(|e| e.to_string())?;

                let wrapped_payload = format!(
                    "<agent_message from=\"{}\" untrusted=\"true\">{}</agent_message>",
                    from_agent, payload_str
                );
                let user_content = serde_json::to_string(&vec![serde_json::json!({
                    "type": "text",
                    "text": wrapped_payload,
                })])
                .map_err(|e| e.to_string())?;
                conn.execute(
                    "INSERT INTO chat_messages (id, session_id, role, content, created_at)
                     VALUES (?1, ?2, 'user', ?3, ?4)",
                    rusqlite::params![ulid::Ulid::new().to_string(), new_session_id, user_content, now],
                )
                .map_err(|e| e.to_string())?;

                conn.execute(
                    "INSERT INTO bus_messages (
                       id, from_agent_id, from_run_id, from_session_id, to_agent_id, to_run_id, to_session_id,
                       kind, payload, status, created_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, 'direct', ?7, 'delivered', ?8)",
                    rusqlite::params![msg_id, from_agent, from_run, from_session, to_agent_id, new_session_id, payload_str, now],
                )
                .map_err(|e| e.to_string())?;

                conn.execute(
                    "UPDATE chat_sessions SET source_bus_message_id = ?1 WHERE id = ?2",
                    rusqlite::params![msg_id, new_session_id],
                )
                .map_err(|e| e.to_string())?;

                Ok::<(), String>(())
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        }

        let db_clone = db.clone();
        let app_clone = bus_app.clone();
        let tx_clone = ctx
            .executor_tx
            .as_ref()
            .ok_or("send_message: executor channel not available")?
            .clone();
        let semaphores = agent_semaphores.clone();
        let registry = session_registry.clone();
        let perm_registry = ctx
            .permission_registry
            .clone()
            .unwrap_or_else(PermissionRegistry::new);
        let mem_client = ctx.memory_client.clone();
        let mem_user_id = ctx.memory_user_id.clone();
        let cloud_cl = ctx.cloud_client.clone();
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
                    cloud_cl,
                )
                .await
                {
                    warn!(session_id = %target_session_id, "send_message session failed: {}", e);
                }
            })
        });

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
                format!(
                    "Message sent to agent '{}'. Session ID: {}. The agent will process your message asynchronously.",
                    to_agent_name, new_session_id
                ),
                false,
            ));
        }

        let timeout = tokio::time::Duration::from_secs(120);
        let start = tokio::time::Instant::now();
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            if start.elapsed() > timeout {
                return Ok((
                    format!(
                        "Timed out waiting for agent '{}' (session {}). The agent may still be running.",
                        to_agent_name, new_session_id
                    ),
                    false,
                ));
            }

            let pool = db.clone();
            let sid = new_session_id.clone();
            let result: Option<(Option<String>, Option<String>, Option<String>)> =
                tokio::task::spawn_blocking(move || {
                    let conn = pool.get().ok()?;
                    conn.query_row(
                        "SELECT execution_state, finish_summary, terminal_error FROM chat_sessions WHERE id = ?1",
                        rusqlite::params![sid],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .ok()
                })
                .await
                .ok()
                .flatten();

            match result {
                Some((Some(state), finish_summary, terminal_error))
                    if matches!(
                        state.as_str(),
                        "success" | "failure" | "cancelled" | "timed_out"
                    ) =>
                {
                    let summary = finish_summary.or(terminal_error).unwrap_or_else(|| {
                        format!("Agent '{}' finished with state: {}", to_agent_name, state)
                    });
                    return Ok((summary, false));
                }
                _ => {}
            }
        }
    }
}
