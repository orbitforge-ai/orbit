use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::json;
use tracing::warn;
use ulid::Ulid;

use crate::db::DbPool;
use crate::events::emitter::emit_bus_message_sent;
use crate::executor::llm_provider::ContentBlock;
use crate::executor::permissions::PermissionRegistry;
use crate::executor::session_agent;
use crate::models::chat::ChatSession;

use super::context::ToolExecutionContext;

pub const MAX_SESSION_CHAIN_DEPTH: i64 = 10;

#[derive(Debug, Clone)]
pub struct ResolvedAgent {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct AccessibleSessionRecord {
    pub session: ChatSession,
    pub allow_sub_agents: bool,
}

#[derive(Debug, Clone)]
pub struct SessionTerminalState {
    pub execution_state: String,
    pub finish_summary: Option<String>,
    pub terminal_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildSessionRecord {
    pub id: String,
    pub agent_id: String,
    pub title: String,
    pub session_type: String,
    pub execution_state: Option<String>,
    pub finish_summary: Option<String>,
    pub terminal_error: Option<String>,
    pub allow_sub_agents: bool,
    pub created_at: String,
    pub updated_at: String,
}

pub fn current_bus_run_id(ctx: &ToolExecutionContext) -> Option<String> {
    ctx.current_run_id.as_ref().and_then(|rid| {
        if rid.starts_with("chat:") {
            None
        } else {
            Some(rid.clone())
        }
    })
}

pub fn wrap_agent_message(from_agent_id: &str, message: &str) -> String {
    format!(
        "<agent_message from=\"{}\" untrusted=\"true\">{}</agent_message>",
        from_agent_id, message
    )
}

pub async fn resolve_agent(
    db: &DbPool,
    requested_agent: Option<&str>,
    default_agent_id: &str,
) -> Result<ResolvedAgent, String> {
    let pool = db.0.clone();
    let lookup = requested_agent.unwrap_or(default_agent_id).to_string();

    tokio::task::spawn_blocking(move || -> Result<ResolvedAgent, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT id, name FROM agents WHERE id = ?1 OR name = ?1 LIMIT 1",
            rusqlite::params![lookup.clone()],
            |row| {
                Ok(ResolvedAgent {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            },
        )
        .map_err(|_| format!("Agent '{}' not found", lookup))
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn create_session_with_initial_message(
    db: &DbPool,
    agent_id: &str,
    session_type: &str,
    title: Option<&str>,
    parent_session_id: Option<&str>,
    chain_depth: i64,
    allow_sub_agents: bool,
    initial_text: &str,
    source_bus_message_id: Option<&str>,
) -> Result<String, String> {
    let pool = db.0.clone();
    let session_id = Ulid::new().to_string();
    let agent_id = agent_id.to_string();
    let session_type = session_type.to_string();
    let title = title
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| truncate_chars(initial_text, 60));
    let parent_session_id = parent_session_id.map(str::to_string);
    let source_bus_message_id = source_bus_message_id.map(str::to_string);
    let initial_text = initial_text.to_string();
    let session_id_for_db = session_id.clone();

    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO chat_sessions (
               id, agent_id, title, archived, session_type, parent_session_id, source_bus_message_id,
               chain_depth, execution_state, finish_summary, terminal_error, created_at, updated_at,
               allow_sub_agents
             ) VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7, 'queued', NULL, NULL, ?8, ?8, ?9)",
            rusqlite::params![
                &session_id_for_db,
                &agent_id,
                &title,
                &session_type,
                &parent_session_id,
                &source_bus_message_id,
                chain_depth,
                &now,
                allow_sub_agents,
            ],
        )
        .map_err(|e| e.to_string())?;

        let user_content = serde_json::to_string(&vec![json!({
            "type": "text",
            "text": initial_text,
        })])
        .map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO chat_messages (id, session_id, role, content, created_at)
             VALUES (?1, ?2, 'user', ?3, ?4)",
            rusqlite::params![
                Ulid::new().to_string(),
                &session_id_for_db,
                user_content,
                &now
            ],
        )
        .map_err(|e| e.to_string())?;

        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;

    Ok(session_id)
}

pub async fn append_user_text_message(
    db: &DbPool,
    session_id: &str,
    message: &str,
    cloud_client: Option<Arc<crate::db::cloud::SupabaseClient>>,
) -> Result<(), String> {
    let blocks = vec![ContentBlock::Text {
        text: message.to_string(),
    }];
    session_agent::save_chat_message(&db.0, session_id, "user", &blocks, cloud_client).await
}

pub async fn load_accessible_session(
    db: &DbPool,
    requester_agent_id: &str,
    session_id: &str,
) -> Result<AccessibleSessionRecord, String> {
    let pool = db.0.clone();
    let requester_agent_id = requester_agent_id.to_string();
    let session_id = session_id.to_string();

    tokio::task::spawn_blocking(move || -> Result<AccessibleSessionRecord, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let row = conn
            .query_row(
                "SELECT id, agent_id, title, archived, session_type, parent_session_id, source_bus_message_id,
                        chain_depth, execution_state, finish_summary, terminal_error, created_at, updated_at,
                        project_id, allow_sub_agents
                 FROM chat_sessions
                 WHERE id = ?1",
                rusqlite::params![session_id.clone()],
                |row| {
                    Ok(AccessibleSessionRecord {
                        session: ChatSession {
                            id: row.get(0)?,
                            agent_id: row.get(1)?,
                            title: row.get(2)?,
                            archived: row.get::<_, bool>(3)?,
                            session_type: row.get(4)?,
                            parent_session_id: row.get(5)?,
                            source_bus_message_id: row.get(6)?,
                            chain_depth: row.get(7)?,
                            execution_state: row.get(8)?,
                            finish_summary: row.get(9)?,
                            terminal_error: row.get(10)?,
                            source_agent_id: None,
                            source_agent_name: None,
                            source_session_id: None,
                            source_session_title: None,
                            created_at: row.get(11)?,
                            updated_at: row.get(12)?,
                            project_id: row.get(13)?,
                        },
                        allow_sub_agents: row.get::<_, bool>(14)?,
                    })
                },
            )
            .map_err(|_| format!("session '{}' not found", session_id))?;

        if row.session.agent_id == requester_agent_id {
            return Ok(row);
        }

        let has_relationship: bool = conn
            .query_row(
                "SELECT EXISTS(
                   SELECT 1
                   FROM bus_messages
                   WHERE (from_agent_id = ?1 AND to_agent_id = ?2)
                      OR (from_agent_id = ?2 AND to_agent_id = ?1)
                 )",
                rusqlite::params![requester_agent_id, row.session.agent_id.clone()],
                |result_row| result_row.get(0),
            )
            .map_err(|e| e.to_string())?;

        if !has_relationship {
            return Err(format!(
                "cannot access session '{}' because it belongs to another agent without an existing bus relationship",
                row.session.id
            ));
        }

        Ok(row)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn record_bus_message(
    db: &DbPool,
    app: &tauri::AppHandle,
    from_agent_id: &str,
    from_run_id: Option<&str>,
    from_session_id: Option<&str>,
    to_agent_id: &str,
    to_session_id: Option<&str>,
    payload: &str,
) -> Result<String, String> {
    let pool = db.0.clone();
    let message_id = Ulid::new().to_string();
    let from_agent_id = from_agent_id.to_string();
    let from_run_id = from_run_id.map(str::to_string);
    let from_session_id = from_session_id.map(str::to_string);
    let to_agent_id = to_agent_id.to_string();
    let to_session_id = to_session_id.map(str::to_string);
    let payload_text = truncate_bytes(payload, 50_000);
    let message_id_for_db = message_id.clone();
    let from_agent_id_for_db = from_agent_id.clone();
    let to_agent_id_for_db = to_agent_id.clone();
    let to_session_for_event = to_session_id.clone();

    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO bus_messages (
               id, from_agent_id, from_run_id, from_session_id, to_agent_id, to_run_id, to_session_id,
               kind, payload, status, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, 'direct', ?7, 'delivered', ?8)",
            rusqlite::params![
                &message_id_for_db,
                &from_agent_id_for_db,
                &from_run_id,
                &from_session_id,
                &to_agent_id_for_db,
                &to_session_id,
                &payload_text,
                &now,
            ],
        )
        .map_err(|e| e.to_string())?;

        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;

    emit_bus_message_sent(
        app,
        &message_id,
        &from_agent_id,
        &to_agent_id,
        "direct",
        json!({ "message": payload }),
        to_session_for_event.as_deref(),
        None,
    );

    Ok(message_id)
}

pub async fn update_session_source_bus_message(
    db: &DbPool,
    session_id: &str,
    message_id: &str,
) -> Result<(), String> {
    let pool = db.0.clone();
    let session_id = session_id.to_string();
    let message_id = message_id.to_string();

    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE chat_sessions SET source_bus_message_id = ?1 WHERE id = ?2",
            rusqlite::params![message_id, session_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn update_session_chain_depth(
    db: &DbPool,
    session_id: &str,
    chain_depth: i64,
) -> Result<(), String> {
    let pool = db.0.clone();
    let session_id = session_id.to_string();

    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE chat_sessions SET chain_depth = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![chain_depth, now, session_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn load_execution_state(db: &DbPool, session_id: &str) -> Result<Option<String>, String> {
    let pool = db.0.clone();
    let session_id = session_id.to_string();

    tokio::task::spawn_blocking(move || -> Result<Option<String>, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT execution_state FROM chat_sessions WHERE id = ?1",
            rusqlite::params![session_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn wait_for_session_terminal(
    db: &DbPool,
    session_id: &str,
    timeout: Duration,
) -> Result<Option<SessionTerminalState>, String> {
    let started = Instant::now();

    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;

        if started.elapsed() > timeout {
            return Ok(None);
        }

        let pool = db.0.clone();
        let session_id = session_id.to_string();
        let state = tokio::task::spawn_blocking(move || -> Result<SessionTerminalState, String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            conn.query_row(
                "SELECT COALESCE(execution_state, 'queued'), finish_summary, terminal_error
                 FROM chat_sessions WHERE id = ?1",
                rusqlite::params![session_id],
                |row| {
                    Ok(SessionTerminalState {
                        execution_state: row.get(0)?,
                        finish_summary: row.get(1)?,
                        terminal_error: row.get(2)?,
                    })
                },
            )
            .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??;

        if is_terminal_state(&state.execution_state) {
            return Ok(Some(state));
        }
    }
}

pub async fn list_child_sessions(
    db: &DbPool,
    parent_session_id: &str,
) -> Result<Vec<ChildSessionRecord>, String> {
    let pool = db.0.clone();
    let parent_session_id = parent_session_id.to_string();

    tokio::task::spawn_blocking(move || -> Result<Vec<ChildSessionRecord>, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, agent_id, title, session_type, execution_state, finish_summary,
                        terminal_error, allow_sub_agents, created_at, updated_at
                 FROM chat_sessions
                 WHERE parent_session_id = ?1
                 ORDER BY created_at ASC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(rusqlite::params![parent_session_id], |row| {
                Ok(ChildSessionRecord {
                    id: row.get(0)?,
                    agent_id: row.get(1)?,
                    title: row.get(2)?,
                    session_type: row.get(3)?,
                    execution_state: row.get(4)?,
                    finish_summary: row.get(5)?,
                    terminal_error: row.get(6)?,
                    allow_sub_agents: row.get::<_, bool>(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|row| row.ok())
            .collect();

        Ok(rows)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn start_session_run(
    ctx: &ToolExecutionContext,
    target_agent_id: &str,
    session_id: &str,
    chain_depth: i64,
    is_sub_agent: bool,
    allow_sub_agents: bool,
) -> Result<(), String> {
    let db = ctx
        .db
        .as_ref()
        .ok_or("session run: database not available")?
        .clone();
    let app = ctx
        .app
        .as_ref()
        .ok_or("session run: app handle not available")?
        .clone();
    let executor_tx = ctx
        .executor_tx
        .as_ref()
        .ok_or("session run: executor channel not available")?
        .clone();
    let agent_semaphores = ctx
        .agent_semaphores
        .as_ref()
        .ok_or("session run: agent semaphores not available")?
        .clone();
    let session_registry = ctx
        .session_registry
        .as_ref()
        .ok_or("session run: session registry not available")?
        .clone();
    let permission_registry = ctx
        .permission_registry
        .clone()
        .unwrap_or_else(PermissionRegistry::new);
    let user_question_registry = ctx.user_question_registry.clone();
    let memory_client = ctx.memory_client.clone();
    let memory_user_id = ctx.memory_user_id.clone();
    let cloud_client = ctx.cloud_client.clone();
    let target_agent_id = target_agent_id.to_string();
    let session_id = session_id.to_string();

    tokio::task::spawn_blocking(move || {
        tauri::async_runtime::block_on(async move {
            if let Err(error) = session_agent::run_agent_session(
                &target_agent_id,
                &session_id,
                chain_depth,
                is_sub_agent,
                allow_sub_agents,
                &db,
                &app,
                &executor_tx,
                &agent_semaphores,
                &session_registry,
                &permission_registry,
                user_question_registry.as_ref(),
                memory_client.as_ref(),
                &memory_user_id,
                cloud_client,
            )
            .await
            {
                warn!(session_id = %session_id, "session run failed: {}", error);
            }
        })
    });

    Ok(())
}

pub fn is_terminal_state(state: &str) -> bool {
    matches!(state, "success" | "failure" | "cancelled" | "timed_out")
}

pub fn is_active_state(state: &str) -> bool {
    matches!(
        state,
        "queued"
            | "running"
            | "waiting_message"
            | "waiting_user"
            | "waiting_timeout"
            | "waiting_sub_agents"
    )
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }

    let truncated: String = text.chars().take(max_chars).collect();
    format!("{}...", truncated)
}

fn truncate_bytes(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }

    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}
