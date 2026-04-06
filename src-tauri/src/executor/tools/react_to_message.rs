use serde_json::json;

use crate::executor::llm_provider::ToolDefinition;

use super::{context::ToolExecutionContext, ToolHandler};

pub struct ReactToMessageTool;

#[async_trait::async_trait]
impl ToolHandler for ReactToMessageTool {
    fn name(&self) -> &'static str {
        "react_to_message"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "React to a user message with an emoji. Use this to express appreciation, acknowledgment, or emotion about what the user said. Use sparingly and genuinely. Reference the message ID from the Message IDs list in your context.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message_id": {
                        "type": "string",
                        "description": "The ID of the user message to react to (from the Message IDs list in context)"
                    },
                    "emoji": {
                        "type": "string",
                        "description": "A single emoji character to react with",
                        "enum": ["\u{1F44D}", "\u{2764}\u{FE0F}", "\u{1F602}", "\u{1F389}", "\u{1F914}", "\u{1F440}", "\u{1F525}", "\u{1F4AF}", "\u{2705}", "\u{2B50}"]
                    }
                },
                "required": ["message_id", "emoji"]
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
        let message_id = input["message_id"]
            .as_str()
            .ok_or("react_to_message: missing 'message_id'")?;
        let emoji = input["emoji"]
            .as_str()
            .ok_or("react_to_message: missing 'emoji'")?;

        let allowed_emojis = [
            "\u{1F44D}",
            "\u{2764}\u{FE0F}",
            "\u{1F602}",
            "\u{1F389}",
            "\u{1F914}",
            "\u{1F440}",
            "\u{1F525}",
            "\u{1F4AF}",
            "\u{2705}",
            "\u{2B50}",
        ];
        if !allowed_emojis.contains(&emoji) {
            return Err(format!("react_to_message: invalid emoji '{}'", emoji));
        }

        let session_id = ctx
            .current_session_id
            .as_deref()
            .ok_or("react_to_message: no active session")?;

        let db = ctx.db.as_ref().ok_or("react_to_message: no database")?;
        let pool = db.0.clone();
        let reaction_id = ulid::Ulid::new().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let msg_id = message_id.to_string();
        let emoji_str = emoji.to_string();
        let sid = session_id.to_string();
        let rid = reaction_id.clone();
        let now_clone = now.clone();

        let inserted = tokio::task::spawn_blocking(move || -> Result<bool, String> {
            let conn = pool.get().map_err(|e| e.to_string())?;
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM chat_messages WHERE id = ?1 AND session_id = ?2 AND role = 'user'",
                    rusqlite::params![msg_id, sid],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if !exists {
                return Err(format!(
                    "Message '{}' not found or not a user message in this session",
                    msg_id
                ));
            }
            let changes = conn
                .execute(
                    "INSERT OR IGNORE INTO message_reactions (id, message_id, session_id, emoji, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![rid, msg_id, sid, emoji_str, now_clone],
                )
                .map_err(|e| e.to_string())?;
            Ok(changes > 0)
        })
        .await
        .map_err(|e| e.to_string())??;

        if inserted {
            if let Some(app_handle) = &ctx.app {
                crate::events::emitter::emit_message_reaction(
                    app_handle,
                    session_id,
                    message_id,
                    &reaction_id,
                    emoji,
                    &now,
                );
            }

            if let Some(cloud) = &ctx.cloud_client {
                let cloud = cloud.clone();
                let rid = reaction_id.clone();
                let mid = message_id.to_string();
                let sid = session_id.to_string();
                let emoji_c = emoji.to_string();
                let now_c = now.clone();
                tokio::spawn(async move {
                    if let Err(e) = cloud
                        .upsert_message_reaction(&rid, &mid, &sid, &emoji_c, &now_c)
                        .await
                    {
                        tracing::warn!("cloud sync reaction failed: {}", e);
                    }
                });
            }
        }

        Ok((format!("Reacted with {} to message", emoji), false))
    }
}
