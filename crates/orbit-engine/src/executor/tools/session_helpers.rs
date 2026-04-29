use serde::Serialize;

use crate::db::DbPool;
use crate::executor::llm_provider::ContentBlock;
use crate::models::chat::ChatSession;

#[derive(Debug, Clone)]
pub struct SessionToolSession {
    pub session: ChatSession,
    pub last_input_tokens: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionToolMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: Option<String>,
    pub is_compacted: bool,
}

#[derive(Debug, Clone)]
pub struct SessionListRecord {
    pub session: ChatSession,
    pub last_input_tokens: Option<i64>,
    pub last_message_preview: Option<String>,
    pub last_message_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessageStats {
    pub message_count: i64,
    pub user_message_count: i64,
    pub assistant_message_count: i64,
    pub compacted_message_count: i64,
    pub last_message_at: Option<String>,
}

pub fn resolve_session_id(
    current_session_id: Option<&str>,
    requested_session_id: Option<&str>,
    tool_name: &str,
) -> Result<String, String> {
    let session_id = requested_session_id.unwrap_or("current");
    if session_id == "current" {
        current_session_id
            .map(str::to_string)
            .ok_or_else(|| format!("{}: no current session", tool_name))
    } else {
        Ok(session_id.to_string())
    }
}

pub async fn load_owned_session(
    db: &DbPool,
    agent_id: &str,
    session_id: &str,
) -> Result<SessionToolSession, String> {
    let pool = db.0.clone();
    let agent_id = agent_id.to_string();
    let session_id = session_id.to_string();

    tokio::task::spawn_blocking(move || -> Result<SessionToolSession, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let row = conn
            .query_row(
                "SELECT id, agent_id, title, archived, session_type, parent_session_id, source_bus_message_id,
                        chain_depth, execution_state, finish_summary, terminal_error, created_at, updated_at,
                        project_id, worktree_name, worktree_branch, worktree_path, last_input_tokens
                 FROM chat_sessions
                 WHERE id = ?1
                   AND tenant_id = COALESCE((SELECT tenant_id FROM agents WHERE id = ?2), 'local')",
                rusqlite::params![session_id, agent_id],
                |row| {
                    Ok(SessionToolSession {
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
                            worktree_name: row.get(14)?,
                            worktree_branch: row.get(15)?,
                            worktree_path: row.get(16)?,
                        },
                        last_input_tokens: row.get(17)?,
                    })
                },
            )
            .map_err(|_| format!("session '{}' not found", session_id))?;

        if row.session.agent_id != agent_id {
            return Err("cannot access sessions from other agents".to_string());
        }

        Ok(row)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn list_session_messages(
    db: &DbPool,
    session_id: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<SessionToolMessage>, String> {
    let pool = db.0.clone();
    let session_id = session_id.to_string();

    tokio::task::spawn_blocking(move || -> Result<Vec<SessionToolMessage>, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, role, content, created_at, is_compacted
                 FROM (
                   SELECT id, role, content, created_at, is_compacted
                   FROM chat_messages
                   WHERE session_id = ?1
                     AND tenant_id = COALESCE((SELECT tenant_id FROM chat_sessions WHERE id = ?1), 'local')
                   ORDER BY created_at DESC
                   LIMIT ?2 OFFSET ?3
                 ) sub
                 ORDER BY created_at ASC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(rusqlite::params![session_id, limit, offset], |row| {
                let content_json: String = row.get(2)?;
                Ok(SessionToolMessage {
                    id: row.get(0)?,
                    role: row.get(1)?,
                    content: content_preview_from_json(&content_json, 500),
                    created_at: row.get(3)?,
                    is_compacted: row.get::<_, bool>(4)?,
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

pub async fn session_message_stats(
    db: &DbPool,
    session_id: &str,
) -> Result<SessionMessageStats, String> {
    let pool = db.0.clone();
    let session_id = session_id.to_string();

    tokio::task::spawn_blocking(move || -> Result<SessionMessageStats, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT
                COUNT(*) AS message_count,
                SUM(CASE WHEN role = 'user' THEN 1 ELSE 0 END) AS user_count,
                SUM(CASE WHEN role = 'assistant' THEN 1 ELSE 0 END) AS assistant_count,
                SUM(CASE WHEN is_compacted = 1 THEN 1 ELSE 0 END) AS compacted_count,
                MAX(created_at) AS last_message_at
             FROM chat_messages
             WHERE session_id = ?1
               AND tenant_id = COALESCE((SELECT tenant_id FROM chat_sessions WHERE id = ?1), 'local')",
            rusqlite::params![session_id],
            |row| {
                Ok(SessionMessageStats {
                    message_count: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                    user_message_count: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    assistant_message_count: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    compacted_message_count: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    last_message_at: row.get(4)?,
                })
            },
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn list_owned_sessions(
    db: &DbPool,
    agent_id: &str,
    session_type: Option<&str>,
    state: Option<&str>,
    search: Option<&str>,
    limit: i64,
) -> Result<Vec<SessionListRecord>, String> {
    let pool = db.0.clone();
    let agent_id = agent_id.to_string();
    let session_type = session_type.map(str::to_string);
    let state = state.map(str::to_string);
    let search = search.map(str::to_string);

    tokio::task::spawn_blocking(move || -> Result<Vec<SessionListRecord>, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let mut sql = String::from(
            "SELECT
                cs.id, cs.agent_id, cs.title, cs.archived, cs.session_type, cs.parent_session_id,
                cs.source_bus_message_id, cs.chain_depth, cs.execution_state, cs.finish_summary,
                cs.terminal_error, cs.created_at, cs.updated_at, cs.project_id,
                cs.worktree_name, cs.worktree_branch, cs.worktree_path, cs.last_input_tokens,
                (
                  SELECT content
                  FROM chat_messages cm
                  WHERE cm.session_id = cs.id AND cm.tenant_id = cs.tenant_id
                  ORDER BY cm.created_at DESC
                  LIMIT 1
                ) AS last_content,
                (
                  SELECT created_at
                  FROM chat_messages cm
                  WHERE cm.session_id = cs.id AND cm.tenant_id = cs.tenant_id
                  ORDER BY cm.created_at DESC
                  LIMIT 1
                ) AS last_message_at
             FROM chat_sessions cs
             WHERE cs.agent_id = ?1
               AND cs.tenant_id = COALESCE((SELECT tenant_id FROM agents WHERE id = ?1), 'local')
               AND cs.archived = 0",
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(agent_id)];

        if let Some(session_type) = session_type {
            sql.push_str(&format!(" AND cs.session_type = ?{}", params.len() + 1));
            params.push(Box::new(session_type));
        }
        if let Some(state) = state {
            sql.push_str(&format!(" AND cs.execution_state = ?{}", params.len() + 1));
            params.push(Box::new(state));
        }
        if let Some(search) = search {
            sql.push_str(&format!(" AND cs.title LIKE ?{}", params.len() + 1));
            params.push(Box::new(format!("%{}%", search)));
        }

        sql.push_str(&format!(
            " ORDER BY cs.updated_at DESC LIMIT ?{}",
            params.len() + 1
        ));
        params.push(Box::new(limit));

        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params.iter().map(|value| value.as_ref()).collect();
        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                let last_content: Option<String> = row.get(18)?;
                Ok(SessionListRecord {
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
                        worktree_name: row.get(14)?,
                        worktree_branch: row.get(15)?,
                        worktree_path: row.get(16)?,
                    },
                    last_input_tokens: row.get(17)?,
                    last_message_preview: last_content
                        .as_ref()
                        .map(|content| content_preview_from_json(content, 160)),
                    last_message_at: row.get(19)?,
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

pub fn content_preview_from_json(content_json: &str, max_chars: usize) -> String {
    let content: Vec<ContentBlock> = serde_json::from_str(content_json).unwrap_or_default();
    content_preview(&content, max_chars)
}

pub fn content_preview(content: &[ContentBlock], max_chars: usize) -> String {
    let mut parts = Vec::new();
    for block in content {
        match block {
            ContentBlock::Text { text } => {
                let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if !normalized.is_empty() {
                    parts.push(normalized);
                }
            }
            ContentBlock::Thinking { .. } => parts.push("[thinking]".to_string()),
            ContentBlock::ToolUse { name, .. } => parts.push(format!("[tool:{}]", name)),
            ContentBlock::ToolResult { is_error, .. } => {
                parts.push(if *is_error {
                    "[tool-error]".to_string()
                } else {
                    "[tool-result]".to_string()
                });
            }
            ContentBlock::Image { .. } => parts.push("[image]".to_string()),
        }
    }

    let combined = if parts.is_empty() {
        "[empty]".to_string()
    } else {
        parts.join(" ")
    };

    truncate_chars(&combined, max_chars)
}

pub fn estimate_input_cost_usd(model: &str, input_tokens: u32) -> Option<f64> {
    let rate_per_million = match model {
        "claude-opus-4-7" | "claude-opus-4-6" | "claude-opus-4-20250514" => 5.0,
        "claude-sonnet-4-6" | "claude-sonnet-4-20250514" | "claude-3-5-sonnet-20241022" => 3.0,
        "claude-haiku-4-5-20251001" | "claude-3-5-haiku-20241022" => 1.0,
        "MiniMax-M2.7" | "MiniMax-M2.7-highspeed" => 0.6,
        "MiniMax-M2.5" | "MiniMax-M2.5-highspeed" => 0.6,
        "MiniMax-M2.1" | "MiniMax-M2.1-highspeed" => 0.6,
        "MiniMax-M2" => 0.6,
        _ => return None,
    };

    Some(((input_tokens as f64 / 1_000_000.0) * rate_per_million * 100_000.0).round() / 100_000.0)
}

pub fn duration_seconds(started_at: &str, ended_at: &str) -> Option<i64> {
    let start = chrono::DateTime::parse_from_rfc3339(started_at).ok()?;
    let end = chrono::DateTime::parse_from_rfc3339(ended_at).ok()?;
    Some((end - start).num_seconds().max(0))
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }

    let truncated: String = text.chars().take(max_chars).collect();
    format!("{}...", truncated)
}

#[cfg(test)]
mod tests {
    use super::{content_preview, estimate_input_cost_usd};
    use crate::executor::llm_provider::ContentBlock;

    #[test]
    fn content_preview_formats_mixed_blocks() {
        let preview = content_preview(
            &[
                ContentBlock::ToolUse {
                    id: "1".to_string(),
                    name: "read_file".to_string(),
                    input: serde_json::json!({}),
                },
                ContentBlock::Text {
                    text: "Hello   world".to_string(),
                },
            ],
            80,
        );
        assert_eq!(preview, "[tool:read_file] Hello world");
    }

    #[test]
    fn cost_estimate_handles_known_models() {
        assert!(estimate_input_cost_usd("claude-sonnet-4-6", 1000).is_some());
        assert!(estimate_input_cost_usd("unknown", 1000).is_none());
    }
}
