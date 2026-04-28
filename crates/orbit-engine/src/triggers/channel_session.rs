//! Persistent mapping between an external trigger channel (plugin + channel
//! + optional thread) and the `chat_session` that holds the conversation
//! with the target agent.
//!
//! Every inbound Discord / Slack / etc. message for a given agent in a
//! given place flows into the same session, so the agent has full history
//! and the standard auto-compaction machinery (`session_agent::run_session_loop`)
//! keeps tokens bounded.

use rusqlite::OptionalExtension;
use serde_json::json;
use ulid::Ulid;

use crate::db::DbPool;
use crate::executor::llm_provider::ContentBlock;
use crate::models::trigger_event::TriggerEventPayload;
use crate::plugins;

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

/// If this session is bound to an external channel, forward the given
/// assistant content blocks' text to that channel via the originating
/// plugin's `send_message` tool. No-op for non-channel sessions (regular
/// chat, pulse, workflow), so the caller can invoke it unconditionally
/// after every assistant turn. Best-effort: failures are logged but never
/// propagated, so a Discord/Slack outage never breaks the session loop.
pub async fn forward_assistant_if_channel(
    app: &tauri::AppHandle,
    db: &DbPool,
    session_id: &str,
    content: &[ContentBlock],
) {
    let text = extract_text(content);
    if text.trim().is_empty() {
        return;
    }

    let binding = match load_binding(db, session_id).await {
        Ok(Some(b)) => b,
        Ok(None) => return,
        Err(err) => {
            tracing::warn!(session_id, "load channel binding: {}", err);
            return;
        }
    };

    let manager = plugins::from_state(app);
    let Some(manifest) = manager.manifest(&binding.plugin_id) else {
        tracing::warn!(plugin_id = %binding.plugin_id, "channel forward: plugin not installed");
        return;
    };
    if !manager.is_enabled(&binding.plugin_id) {
        tracing::warn!(plugin_id = %binding.plugin_id, "channel forward: plugin disabled");
        return;
    }
    if !manifest.tools.iter().any(|t| t.name == "send_message") {
        tracing::warn!(plugin_id = %binding.plugin_id, "channel forward: plugin lacks send_message");
        return;
    }

    let thread_id = if binding.provider_thread_id.is_empty() {
        None
    } else {
        Some(binding.provider_thread_id.clone())
    };
    let args = json!({
        "channelId": binding.provider_channel_id,
        "threadId": thread_id,
        "text": text,
    });
    let extra_env = plugins::oauth::build_env_for_subprocess(&manifest);
    if let Err(err) = manager
        .runtime
        .call_tool(&manifest, "send_message", &args, &extra_env)
        .await
    {
        tracing::warn!(plugin_id = %binding.plugin_id, "channel forward send_message failed: {}", err);
    }

    // Clear typing indicator as soon as the reply lands (matches the
    // behavior of the `message` tool when it routes through a plugin).
    if manifest.tools.iter().any(|t| t.name == "stop_typing") {
        let stop_args = json!({
            "channelId": binding.provider_channel_id,
            "threadId": thread_id,
        });
        if let Err(err) = manager
            .runtime
            .call_tool(&manifest, "stop_typing", &stop_args, &extra_env)
            .await
        {
            tracing::warn!(plugin_id = %binding.plugin_id, "channel forward stop_typing failed: {}", err);
        }
    }
}

struct ChannelBindingRow {
    plugin_id: String,
    provider_channel_id: String,
    provider_thread_id: String,
}

async fn load_binding(db: &DbPool, session_id: &str) -> Result<Option<ChannelBindingRow>, String> {
    let pool = db.0.clone();
    let sid = session_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<Option<ChannelBindingRow>, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT plugin_id, provider_channel_id, provider_thread_id
               FROM channel_sessions WHERE session_id = ?1",
            rusqlite::params![sid],
            |row| {
                Ok(ChannelBindingRow {
                    plugin_id: row.get(0)?,
                    provider_channel_id: row.get(1)?,
                    provider_thread_id: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn extract_text(content: &[ContentBlock]) -> String {
    let mut out = String::new();
    for block in content {
        if let ContentBlock::Text { text } = block {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(text);
        }
    }
    out
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
