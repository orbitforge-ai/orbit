//! Persistent mapping between an external trigger channel (plugin + channel
//! + optional thread) and the `chat_session` that holds the conversation
//! with the target agent.
//!
//! Every inbound Discord / Slack / etc. message for a given agent in a
//! given place flows into the same session, so the agent has full history
//! and the standard auto-compaction machinery (`session_agent::run_session_loop`)
//! keeps tokens bounded.

use rusqlite::OptionalExtension;
use ulid::Ulid;

use crate::db::DbPool;
use crate::models::trigger_event::TriggerEventPayload;

/// Find the chat session that owns the conversation for this `(agent, plugin,
/// channel, thread)` tuple, creating one on first use. Returns the session id.
pub async fn resolve_session_id(
    db: &DbPool,
    agent_id: &str,
    event: &TriggerEventPayload,
) -> Result<String, String> {
    let pool = db.0.clone();
    let agent_id = agent_id.to_string();
    let plugin_id = event.plugin_id.clone();
    let channel_id = event.channel.id.clone();
    let thread_id = event.channel.thread_id.clone().unwrap_or_default();
    let title = default_title(event);

    tokio::task::spawn_blocking(move || -> Result<String, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;

        if let Some(sid) = conn
            .query_row(
                "SELECT session_id FROM channel_sessions
                   WHERE agent_id = ?1 AND plugin_id = ?2
                     AND provider_channel_id = ?3 AND provider_thread_id = ?4",
                rusqlite::params![agent_id, plugin_id, channel_id, thread_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
        {
            return Ok(sid);
        }

        let now = chrono::Utc::now().to_rfc3339();
        let session_id = Ulid::new().to_string();

        conn.execute(
            "INSERT INTO chat_sessions (
                 id, agent_id, title, archived, session_type, parent_session_id,
                 source_bus_message_id, chain_depth, execution_state,
                 finish_summary, terminal_error, created_at, updated_at
               ) VALUES (?1, ?2, ?3, 0, 'channel', NULL, NULL, 0, NULL, NULL, NULL, ?4, ?4)",
            rusqlite::params![session_id, agent_id, title, now],
        )
        .map_err(|e| format!("insert chat_sessions: {}", e))?;

        conn.execute(
            "INSERT INTO channel_sessions (
                 agent_id, plugin_id, provider_channel_id, provider_thread_id,
                 session_id, created_at, updated_at
               ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            rusqlite::params![agent_id, plugin_id, channel_id, thread_id, session_id, now],
        )
        .map_err(|e| format!("insert channel_sessions: {}", e))?;

        Ok(session_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn default_title(event: &TriggerEventPayload) -> String {
    let base = event
        .channel
        .name
        .as_deref()
        .map(|n| format!("#{}", n))
        .unwrap_or_else(|| event.channel.id.clone());
    match event.channel.thread_id.as_deref() {
        Some(thread) => format!("{} · thread {}", base, thread),
        None => base,
    }
}
