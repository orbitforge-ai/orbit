use futures::StreamExt;
use serde_json::json;
use tracing::debug;

use crate::events::emitter::{emit_agent_content_block, emit_agent_llm_chunk, emit_log_chunk};
use crate::executor::llm_provider::{
    ChatMessage, ContentBlock, LlmConfig, LlmProvider, LlmResponse, StopReason, ToolDefinition,
    Usage,
};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic Messages API provider.
pub struct AnthropicProvider {
    api_key: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    /// Convert provider-agnostic messages to Anthropic API format.
    fn serialize_messages(messages: &[ChatMessage]) -> Vec<serde_json::Value> {
        messages
            .iter()
            .map(|msg| {
                let content: Vec<serde_json::Value> = msg
                    .content
                    .iter()
                    .map(|block| match block {
                        ContentBlock::Text { text } => json!({
                            "type": "text",
                            "text": text,
                        }),
                        ContentBlock::Thinking { thinking } => json!({
                            "type": "thinking",
                            "thinking": thinking,
                        }),
                        ContentBlock::ToolUse { id, name, input } => json!({
                            "type": "tool_use",
                            "id": id,
                            "name": name,
                            "input": input,
                        }),
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => json!({
                            "type": "tool_result",
                            "tool_use_id": tool_use_id,
                            "content": content,
                            "is_error": is_error,
                        }),
                        ContentBlock::Image { media_type, data } => json!({
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": media_type,
                                "data": data,
                            }
                        }),
                    })
                    .collect();

                json!({
                    "role": msg.role,
                    "content": content,
                })
            })
            .collect()
    }

    /// Convert provider-agnostic tool definitions to Anthropic format.
    fn serialize_tools(tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            })
            .collect()
    }

    /// Parse a complete SSE stream and return the assembled response.
    async fn parse_sse_stream(
        &self,
        response: reqwest::Response,
        app: &tauri::AppHandle,
        run_id: &str,
        iteration: u32,
    ) -> Result<LlmResponse, String> {
        let mut stream = response.bytes_stream();

        let mut content_blocks: Vec<ContentBlock> = Vec::new();
        let mut stop_reason = StopReason::EndTurn;
        let mut usage = Usage::default();

        // SSE parsing state
        let mut buffer = String::new();
        // Track current content block being built
        let mut current_text = String::new();
        let mut current_thinking = String::new();
        let mut current_tool_id = String::new();
        let mut current_tool_name = String::new();
        let mut current_tool_input_json = String::new();
        let mut current_block_type: Option<String> = None;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| format!("stream error: {}", e))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Process complete SSE lines
            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim_end_matches('\r').to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() || line.starts_with(':') {
                    continue;
                }

                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        continue;
                    }

                    let event: serde_json::Value = match serde_json::from_str(data) {
                        Ok(v) => v,
                        Err(e) => {
                            debug!("failed to parse SSE data: {} — {}", e, data);
                            continue;
                        }
                    };

                    let event_type = event["type"].as_str().unwrap_or("");

                    match event_type {
                        "content_block_start" => {
                            let block = &event["content_block"];
                            let block_type = block["type"].as_str().unwrap_or("");
                            current_block_type = Some(block_type.to_string());

                            match block_type {
                                "text" => {
                                    current_text.clear();
                                }
                                "thinking" => {
                                    current_thinking.clear();
                                }
                                "tool_use" => {
                                    current_tool_id =
                                        block["id"].as_str().unwrap_or("").to_string();
                                    current_tool_name =
                                        block["name"].as_str().unwrap_or("").to_string();
                                    current_tool_input_json.clear();
                                }
                                _ => {}
                            }
                        }
                        "content_block_delta" => {
                            let delta = &event["delta"];
                            let delta_type = delta["type"].as_str().unwrap_or("");

                            match delta_type {
                                "text_delta" => {
                                    if let Some(text) = delta["text"].as_str() {
                                        current_text.push_str(text);

                                        // Stream text delta to frontend
                                        emit_agent_llm_chunk(app, run_id, text, iteration);
                                        // Also emit as log chunk for the log viewer
                                        emit_log_chunk(
                                            app,
                                            run_id,
                                            vec![("stdout".to_string(), text.to_string())],
                                        );
                                    }
                                }
                                "thinking_delta" => {
                                    if let Some(thinking) = delta["thinking"].as_str() {
                                        current_thinking.push_str(thinking);
                                    }
                                }
                                "input_json_delta" => {
                                    if let Some(json_part) = delta["partial_json"].as_str() {
                                        current_tool_input_json.push_str(json_part);
                                    }
                                }
                                _ => {}
                            }
                        }
                        "content_block_stop" => {
                            if let Some(ref bt) = current_block_type {
                                match bt.as_str() {
                                    "text" => {
                                        content_blocks.push(ContentBlock::Text {
                                            text: current_text.clone(),
                                        });
                                    }
                                    "thinking" => {
                                        if !current_thinking.is_empty() {
                                            content_blocks.push(ContentBlock::Thinking {
                                                thinking: current_thinking.clone(),
                                            });
                                            emit_agent_content_block(
                                                app,
                                                run_id,
                                                iteration,
                                                "thinking",
                                                json!({ "type": "thinking", "thinking": current_thinking }),
                                            );
                                        }
                                    }
                                    "tool_use" => {
                                        let input: serde_json::Value =
                                            serde_json::from_str(&current_tool_input_json)
                                                .unwrap_or(json!({}));
                                        content_blocks.push(ContentBlock::ToolUse {
                                            id: current_tool_id.clone(),
                                            name: current_tool_name.clone(),
                                            input: input.clone(),
                                        });
                                        emit_agent_content_block(
                                            app,
                                            run_id,
                                            iteration,
                                            "tool_use",
                                            json!({
                                                "type": "tool_use",
                                                "id": current_tool_id,
                                                "name": current_tool_name,
                                                "input": input,
                                            }),
                                        );
                                    }
                                    _ => {}
                                }
                            }
                            current_block_type = None;
                        }
                        "message_delta" => {
                            if let Some(sr) = event["delta"]["stop_reason"].as_str() {
                                stop_reason = match sr {
                                    "end_turn" => StopReason::EndTurn,
                                    "tool_use" => StopReason::ToolUse,
                                    "max_tokens" => StopReason::MaxTokens,
                                    _ => StopReason::EndTurn,
                                };
                            }
                            if let Some(u) = event["usage"].as_object() {
                                if let Some(out) = u.get("output_tokens").and_then(|v| v.as_u64())
                                {
                                    usage.output_tokens = out as u32;
                                }
                            }
                        }
                        "message_start" => {
                            if let Some(u) = event["message"]["usage"].as_object() {
                                if let Some(inp) =
                                    u.get("input_tokens").and_then(|v| v.as_u64())
                                {
                                    usage.input_tokens = inp as u32;
                                }
                                if let Some(out) =
                                    u.get("output_tokens").and_then(|v| v.as_u64())
                                {
                                    usage.output_tokens = out as u32;
                                }
                            }
                        }
                        "error" => {
                            let err_msg = event["error"]["message"]
                                .as_str()
                                .unwrap_or("unknown API error");
                            return Err(format!("Anthropic API error: {}", err_msg));
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(LlmResponse {
            content: content_blocks,
            stop_reason,
            usage,
        })
    }
}

#[async_trait::async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    async fn chat_streaming(
        &self,
        config: &LlmConfig,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        app: &tauri::AppHandle,
        run_id: &str,
        iteration: u32,
    ) -> Result<LlmResponse, String> {
        let mut body = json!({
            "model": config.model,
            "max_tokens": config.max_tokens,
            "stream": true,
            "messages": Self::serialize_messages(messages),
        });

        if !config.system_prompt.is_empty() {
            body["system"] = json!(config.system_prompt);
        }

        if let Some(temp) = config.temperature {
            body["temperature"] = json!(temp);
        }

        if !tools.is_empty() {
            body["tools"] = json!(Self::serialize_tools(tools));
        }

        let response = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Anthropic request failed: {}", e))?;

        let status = response.status();
        if !status.is_success() && status.as_u16() != 200 {
            // For non-streaming error responses, read the body
            if response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .map(|ct| ct.contains("application/json"))
                .unwrap_or(false)
            {
                let error_body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "unknown error".to_string());
                let error_json: serde_json::Value =
                    serde_json::from_str(&error_body).unwrap_or(json!({"error": error_body}));
                let msg = error_json["error"]["message"]
                    .as_str()
                    .unwrap_or(&error_body);
                return Err(format!("Anthropic API error ({}): {}", status, msg));
            }
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(format!("Anthropic API error ({}): {}", status, error_body));
        }

        self.parse_sse_stream(response, app, run_id, iteration)
            .await
    }
}
