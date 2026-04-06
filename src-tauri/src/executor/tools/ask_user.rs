use std::time::Duration;

use serde_json::json;
use ulid::Ulid;

use crate::events::emitter::emit_user_question;
use crate::executor::{llm_provider::ToolDefinition, session_agent};

use super::{context::ToolExecutionContext, ToolHandler};

pub struct AskUserTool;

#[async_trait::async_trait]
impl ToolHandler for AskUserTool {
    fn name(&self) -> &'static str {
        "ask_user"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Ask the user a question and wait for their response. Use this for clarification, choosing between options, or getting required input before continuing.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "question": {
                        "type": "string",
                        "description": "The question to ask the user."
                    },
                    "choices": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional list of choices."
                    },
                    "allow_custom": {
                        "type": "boolean",
                        "description": "If true and choices are provided, also allow a custom free-text response. Default: true."
                    },
                    "multi_select": {
                        "type": "boolean",
                        "description": "If true, allow selecting multiple choices. Default: false."
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional supporting context shown with the question."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "description": "Optional timeout in seconds. If omitted, waits until the user answers or the session is cancelled."
                    }
                },
                "required": ["question"]
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
        if ctx.is_sub_agent {
            return Ok((
                "Error: Sub-agents cannot ask the user questions directly.".to_string(),
                false,
            ));
        }

        let question = input["question"]
            .as_str()
            .ok_or("ask_user: missing 'question' field")?;
        let choices = input["choices"].as_array().map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(|text| text.to_string()))
                .collect::<Vec<_>>()
        });
        let allow_custom = input["allow_custom"].as_bool().unwrap_or(true);
        let multi_select = input["multi_select"].as_bool().unwrap_or(false);
        let context = input["context"].as_str();
        let timeout_secs = input["timeout_seconds"].as_u64();

        let app = ctx
            .app
            .as_ref()
            .ok_or("ask_user: app handle not available")?;
        let db = ctx.db.as_ref().ok_or("ask_user: no database available")?;
        let session_id = ctx
            .current_session_id
            .as_deref()
            .ok_or("ask_user: no current session available")?;
        let question_registry = ctx
            .user_question_registry
            .as_ref()
            .ok_or("ask_user: question registry not available")?;
        let session_registry = ctx
            .session_registry
            .as_ref()
            .ok_or("ask_user: session registry not available")?;

        let request_id = Ulid::new().to_string();
        let receiver = question_registry
            .register(&request_id, Some(session_id))
            .await;

        session_agent::update_session_execution_state(db, session_id, "waiting_user", None, None)
            .await?;
        emit_user_question(
            app,
            &request_id,
            ctx.current_run_id.as_deref().unwrap_or(""),
            Some(session_id),
            question,
            choices.as_deref(),
            allow_custom,
            multi_select,
            context,
        );

        let answer = if let Some(timeout_secs) = timeout_secs {
            tokio::select! {
                response = receiver => response.map_err(|_| "ask_user: question was cancelled".to_string())?,
                _ = tokio::time::sleep(Duration::from_secs(timeout_secs)) => {
                    question_registry.cancel(&request_id).await;
                    session_agent::update_session_execution_state(db, session_id, "running", None, None).await?;
                    return Ok((json!({
                        "question": question,
                        "response": null,
                        "timedOut": true,
                        "timeoutSeconds": timeout_secs
                    }).to_string(), false));
                }
                _ = wait_for_cancellation(session_registry, session_id) => return Err("cancelled".to_string()),
            }
        } else {
            tokio::select! {
                response = receiver => response.map_err(|_| "ask_user: question was cancelled".to_string())?,
                _ = wait_for_cancellation(session_registry, session_id) => return Err("cancelled".to_string()),
            }
        };

        session_agent::update_session_execution_state(db, session_id, "running", None, None)
            .await?;
        let result = json!({
            "question": question,
            "response": answer,
            "timedOut": false,
        });
        Ok((result.to_string(), false))
    }
}

async fn wait_for_cancellation(
    session_registry: &crate::executor::engine::SessionExecutionRegistry,
    session_id: &str,
) {
    loop {
        if session_registry.is_cancelled(session_id).await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}
