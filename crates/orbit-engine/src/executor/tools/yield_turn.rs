use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::json;

use crate::db::DbPool;
use crate::executor::{llm_provider::ToolDefinition, session_agent};

use super::{
    context::ToolExecutionContext,
    session_control::{is_terminal_state, list_child_sessions},
    session_helpers::content_preview_from_json,
    ToolHandler,
};

const DEFAULT_TIMEOUT_SECS: u64 = 300;
const MAX_TIMEOUT_SECS: u64 = 600;

pub struct YieldTurnTool;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct YieldTurnResult {
    status: String,
    reason: String,
    wait_for: String,
    timed_out: bool,
    details: serde_json::Value,
}

#[async_trait::async_trait]
impl ToolHandler for YieldTurnTool {
    fn name(&self) -> &'static str {
        "yield_turn"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "End your current turn and wait for an async condition before continuing. Use this after spawning sub-agents or when you want to pause for a future message or a timeout.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "reason": {
                        "type": "string",
                        "description": "What you're waiting for."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "description": "Maximum time to wait before resuming. Defaults to 300 and is capped at 600."
                    },
                    "wait_for": {
                        "type": "string",
                        "enum": ["sub_agents", "message", "timeout"],
                        "description": "What should resume the turn. Defaults to 'sub_agents'."
                    }
                }
            }),
        }
    }

    async fn execute(
        &self,
        ctx: &ToolExecutionContext,
        input: &serde_json::Value,
        _app: &tauri::AppHandle,
        _run_id: &str,
    ) -> Result<(String, bool), String> {
        let db = ctx.db.as_ref().ok_or("yield_turn: no database available")?;
        let session_id = ctx
            .current_session_id
            .as_deref()
            .ok_or("yield_turn: no current session available")?;
        let session_registry = ctx
            .session_registry
            .as_ref()
            .ok_or("yield_turn: session registry not available")?;
        let reason = input["reason"]
            .as_str()
            .unwrap_or("Waiting for async work to finish");
        let wait_for = input["wait_for"].as_str().unwrap_or("sub_agents");
        let timeout_secs = input["timeout_seconds"]
            .as_u64()
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .min(MAX_TIMEOUT_SECS);

        let waiting_state = match wait_for {
            "sub_agents" => "waiting_sub_agents",
            "message" => "waiting_message",
            "timeout" => "waiting_timeout",
            other => return Err(format!("yield_turn: unsupported wait_for '{}'", other)),
        };

        session_agent::update_session_execution_state(db, session_id, waiting_state, None, None)
            .await?;

        let result = match wait_for {
            "sub_agents" => {
                wait_for_sub_agents(db, session_id, session_registry, reason, timeout_secs).await?
            }
            "message" => {
                wait_for_new_message(db, session_id, session_registry, reason, timeout_secs).await?
            }
            "timeout" => {
                wait_for_timeout(session_id, session_registry, reason, timeout_secs).await?
            }
            _ => unreachable!(),
        };

        session_agent::update_session_execution_state(db, session_id, "running", None, None)
            .await?;

        let serialized = serde_json::to_string_pretty(&result)
            .map_err(|e| format!("yield_turn: failed to serialize result: {}", e))?;
        Ok((serialized, false))
    }
}

async fn wait_for_sub_agents(
    db: &DbPool,
    session_id: &str,
    session_registry: &crate::executor::engine::SessionExecutionRegistry,
    reason: &str,
    timeout_secs: u64,
) -> Result<YieldTurnResult, String> {
    let initial_children = list_child_sessions(db, session_id).await?;
    if initial_children.is_empty() {
        return Ok(YieldTurnResult {
            status: "resumed".to_string(),
            reason: reason.to_string(),
            wait_for: "sub_agents".to_string(),
            timed_out: false,
            details: json!({
                "note": "No spawned child sessions were found for the current session.",
                "childSessions": []
            }),
        });
    }

    let tracked_ids: Vec<String> = initial_children
        .iter()
        .map(|child| child.id.clone())
        .collect();
    let started = Instant::now();
    loop {
        if session_registry.is_cancelled(session_id).await {
            return Err("cancelled".to_string());
        }

        let children = list_child_sessions(db, session_id).await?;
        let tracked_children: Vec<_> = children
            .into_iter()
            .filter(|child| tracked_ids.contains(&child.id))
            .collect();
        let all_done = tracked_children.iter().all(|child| {
            child
                .execution_state
                .as_deref()
                .map(is_terminal_state)
                .unwrap_or(false)
        });

        if all_done {
            return Ok(YieldTurnResult {
                status: "resumed".to_string(),
                reason: reason.to_string(),
                wait_for: "sub_agents".to_string(),
                timed_out: false,
                details: json!({ "childSessions": tracked_children }),
            });
        }

        if started.elapsed() >= Duration::from_secs(timeout_secs) {
            return Ok(YieldTurnResult {
                status: "timeout".to_string(),
                reason: reason.to_string(),
                wait_for: "sub_agents".to_string(),
                timed_out: true,
                details: json!({ "childSessions": tracked_children }),
            });
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn wait_for_new_message(
    db: &DbPool,
    session_id: &str,
    session_registry: &crate::executor::engine::SessionExecutionRegistry,
    reason: &str,
    timeout_secs: u64,
) -> Result<YieldTurnResult, String> {
    let anchor = latest_message_timestamp(db, session_id).await?;
    let started = Instant::now();

    loop {
        if session_registry.is_cancelled(session_id).await {
            return Err("cancelled".to_string());
        }

        if let Some(message) = next_user_message_after(db, session_id, anchor.as_deref()).await? {
            return Ok(YieldTurnResult {
                status: "resumed".to_string(),
                reason: reason.to_string(),
                wait_for: "message".to_string(),
                timed_out: false,
                details: json!({
                    "message": message
                }),
            });
        }

        if started.elapsed() >= Duration::from_secs(timeout_secs) {
            return Ok(YieldTurnResult {
                status: "timeout".to_string(),
                reason: reason.to_string(),
                wait_for: "message".to_string(),
                timed_out: true,
                details: json!({
                    "note": format!("No new message arrived within {} seconds.", timeout_secs)
                }),
            });
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn wait_for_timeout(
    session_id: &str,
    session_registry: &crate::executor::engine::SessionExecutionRegistry,
    reason: &str,
    timeout_secs: u64,
) -> Result<YieldTurnResult, String> {
    let started = Instant::now();
    loop {
        if session_registry.is_cancelled(session_id).await {
            return Err("cancelled".to_string());
        }

        if started.elapsed() >= Duration::from_secs(timeout_secs) {
            return Ok(YieldTurnResult {
                status: "resumed".to_string(),
                reason: reason.to_string(),
                wait_for: "timeout".to_string(),
                timed_out: false,
                details: json!({
                    "sleptSeconds": timeout_secs
                }),
            });
        }

        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn latest_message_timestamp(db: &DbPool, session_id: &str) -> Result<Option<String>, String> {
    let pool = db.0.clone();
    let session_id = session_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<Option<String>, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT MAX(created_at)
               FROM chat_messages
              WHERE session_id = ?1
                AND tenant_id = COALESCE((SELECT tenant_id FROM chat_sessions WHERE id = ?1), 'local')",
            rusqlite::params![session_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn next_user_message_after(
    db: &DbPool,
    session_id: &str,
    anchor: Option<&str>,
) -> Result<Option<serde_json::Value>, String> {
    let pool = db.0.clone();
    let session_id = session_id.to_string();
    let anchor = anchor.map(|value| value.to_string());
    tokio::task::spawn_blocking(move || -> Result<Option<serde_json::Value>, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let mut sql = String::from(
            "SELECT id, content, created_at
             FROM chat_messages
             WHERE session_id = ?1
               AND tenant_id = COALESCE((SELECT tenant_id FROM chat_sessions WHERE id = ?1), 'local')
               AND role = 'user'",
        );
        if anchor.is_some() {
            sql.push_str(" AND created_at > ?2");
        }
        sql.push_str(" ORDER BY created_at ASC LIMIT 1");

        let row = if let Some(anchor) = anchor {
            conn.query_row(&sql, rusqlite::params![session_id, anchor], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
        } else {
            conn.query_row(&sql, rusqlite::params![session_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
        };

        match row {
            Ok((id, content, created_at)) => Ok(Some(json!({
                "id": id,
                "createdAt": created_at,
                "preview": content_preview_from_json(&content, 240),
            }))),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
