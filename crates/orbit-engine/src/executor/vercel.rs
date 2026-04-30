use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::debug;

use crate::events::emitter::{emit_agent_content_block, emit_agent_llm_chunk, emit_log_chunk};
use crate::executor::llm_provider::{
    sanitize_tool_input_schema, ChatMessage, ContentBlock, LlmConfig, LlmProvider, LlmResponse,
    StopReason, ToolDefinition, Usage,
};

const VERCEL_CHAT_COMPLETIONS_URL: &str = "https://ai-gateway.vercel.sh/v1/chat/completions";
const VERCEL_MODELS_URL: &str = "https://ai-gateway.vercel.sh/v1/models";
const VERCEL_FALLBACK_MODEL: &str = "openai/gpt-5.4";

static MODEL_METADATA_CACHE: OnceLock<Mutex<BTreeMap<String, VercelModelMetadata>>> =
    OnceLock::new();

fn model_cache() -> &'static Mutex<BTreeMap<String, VercelModelMetadata>> {
    MODEL_METADATA_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VercelGatewayModelOption {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VercelModelsResponse {
    data: Vec<VercelModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VercelModel {
    id: String,
    name: Option<String>,
    #[serde(default)]
    owned_by: Option<String>,
    #[serde(default)]
    context_window: Option<u32>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Clone)]
struct VercelModelMetadata {
    context_window: Option<u32>,
    tags: Vec<String>,
}

pub fn fallback_model_option() -> VercelGatewayModelOption {
    VercelGatewayModelOption {
        label: "OpenAI GPT-5.4".to_string(),
        value: VERCEL_FALLBACK_MODEL.to_string(),
    }
}

pub fn cached_model_context_window(model: &str) -> Option<u32> {
    model_cache().lock().ok().and_then(|cache| {
        cache
            .get(model)
            .and_then(|metadata| metadata.context_window)
    })
}

pub fn cached_model_supports_images(model: &str) -> bool {
    model_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(model).cloned())
        .map(|metadata| metadata.tags.iter().any(|tag| tag == "vision"))
        .unwrap_or(false)
}

fn cache_models(models: &[VercelModel]) {
    if let Ok(mut cache) = model_cache().lock() {
        for model in models {
            cache.insert(
                model.id.clone(),
                VercelModelMetadata {
                    context_window: model.context_window,
                    tags: model.tags.clone(),
                },
            );
        }
    }
}

fn model_label(model: &VercelModel) -> String {
    let provider = model
        .owned_by
        .as_deref()
        .map(humanize_provider)
        .unwrap_or_else(|| {
            model
                .id
                .split_once('/')
                .map(|(prefix, _)| humanize_provider(prefix))
                .unwrap_or_else(|| "Vercel".to_string())
        });
    let name = model.name.as_deref().unwrap_or(&model.id);
    format!("{} - {}", provider, name)
}

fn humanize_provider(provider: &str) -> String {
    match provider {
        "openai" => return "OpenAI".to_string(),
        "xai" => return "xAI".to_string(),
        "deepseek" => return "DeepSeek".to_string(),
        _ => {}
    }

    provider
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_tool_capable_language_model(model: &VercelModel) -> bool {
    model.r#type.as_deref() == Some("language") && model.tags.iter().any(|tag| tag == "tool-use")
}

fn parse_model_options(value: Value) -> Result<Vec<VercelGatewayModelOption>, String> {
    let response: VercelModelsResponse =
        serde_json::from_value(value).map_err(|e| format!("Vercel model parse failed: {}", e))?;
    cache_models(&response.data);

    let mut models: Vec<&VercelModel> = response
        .data
        .iter()
        .filter(|model| is_tool_capable_language_model(model))
        .collect();
    models.sort_by(|a, b| {
        let provider_cmp = a.owned_by.cmp(&b.owned_by);
        if provider_cmp == std::cmp::Ordering::Equal {
            a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id))
        } else {
            provider_cmp
        }
    });

    let options = models
        .into_iter()
        .map(|model| VercelGatewayModelOption {
            label: model_label(model),
            value: model.id.clone(),
        })
        .collect::<Vec<_>>();

    if options.is_empty() {
        Ok(vec![fallback_model_option()])
    } else {
        Ok(options)
    }
}

pub async fn list_gateway_models(
    api_key: Option<String>,
) -> Result<Vec<VercelGatewayModelOption>, String> {
    let client = reqwest::Client::new();
    let response = send_models_request(&client, api_key.as_deref()).await?;
    let status = response.status();
    if !status.is_success() {
        if api_key.as_deref().is_some_and(|key| !key.trim().is_empty()) {
            let retry = send_models_request(&client, None).await?;
            if retry.status().is_success() {
                let value = retry
                    .json::<Value>()
                    .await
                    .map_err(|e| format!("Vercel models response parse failed: {}", e))?;
                return parse_model_options(value);
            }
        }
        let error_body = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        return Err(format!(
            "Vercel models API error ({}): {}",
            status, error_body
        ));
    }

    let value = response
        .json::<Value>()
        .await
        .map_err(|e| format!("Vercel models response parse failed: {}", e))?;
    parse_model_options(value)
}

async fn send_models_request(
    client: &reqwest::Client,
    api_key: Option<&str>,
) -> Result<reqwest::Response, String> {
    let mut request = client
        .get(VERCEL_MODELS_URL)
        .header("content-type", "application/json");
    if let Some(key) = api_key.filter(|key| !key.trim().is_empty()) {
        request = request.bearer_auth(key);
    }

    request
        .send()
        .await
        .map_err(|e| format!("Vercel models request failed: {}", e))
}

#[derive(Debug, Clone, Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

struct ToolInputDelta {
    id: String,
    name: String,
    partial_json: String,
}

/// Vercel AI Gateway provider using its OpenAI-compatible Chat Completions API.
pub struct VercelProvider {
    api_key: String,
    client: reqwest::Client,
}

impl VercelProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    fn serialize_messages(config: &LlmConfig, messages: &[ChatMessage]) -> Vec<Value> {
        let mut serialized = Vec::new();
        if !config.system_prompt.is_empty() {
            serialized.push(json!({
                "role": "system",
                "content": config.system_prompt,
            }));
        }

        for message in messages {
            serialized.extend(Self::serialize_message(message));
        }

        serialized
    }

    fn serialize_message(message: &ChatMessage) -> Vec<Value> {
        let mut content_parts = Vec::new();
        let mut tool_calls = Vec::new();
        let mut tool_result_messages = Vec::new();

        for block in &message.content {
            match block {
                ContentBlock::Text { text } => {
                    if !text.is_empty() {
                        content_parts.push(json!({ "type": "text", "text": text }));
                    }
                }
                ContentBlock::Image { media_type, data } => {
                    content_parts.push(json!({
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:{};base64,{}", media_type, data),
                        },
                    }));
                }
                ContentBlock::ToolUse { id, name, input } => {
                    tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string()),
                        },
                    }));
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    let text = if *is_error {
                        format!("Error: {}", content)
                    } else {
                        content.clone()
                    };
                    tool_result_messages.push(json!({
                        "role": "tool",
                        "tool_call_id": tool_use_id,
                        "content": text,
                    }));
                }
                ContentBlock::Thinking { .. } => {
                    // OpenAI-compatible chat does not accept provider-native
                    // thinking blocks in history.
                }
            }
        }

        let mut out = Vec::new();
        if message.role == "assistant" && !tool_calls.is_empty() {
            let content = Self::serialize_chat_content(content_parts);
            out.push(json!({
                "role": "assistant",
                "content": content,
                "tool_calls": tool_calls,
            }));
        } else if !content_parts.is_empty() {
            out.push(json!({
                "role": message.role,
                "content": Self::serialize_chat_content(content_parts),
            }));
        }

        out.extend(tool_result_messages);
        out
    }

    fn serialize_chat_content(parts: Vec<Value>) -> Value {
        if parts.len() == 1 && parts[0]["type"].as_str() == Some("text") {
            parts[0]["text"].clone()
        } else if parts.is_empty() {
            Value::String(String::new())
        } else {
            Value::Array(parts)
        }
    }

    fn serialize_tools(tools: &[ToolDefinition]) -> Vec<Value> {
        tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": sanitize_tool_input_schema(&tool.input_schema),
                    },
                })
            })
            .collect()
    }

    fn request_body(
        config: &LlmConfig,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Value {
        let mut body = json!({
            "model": config.model,
            "max_tokens": config.max_tokens,
            "messages": Self::serialize_messages(config, messages),
        });

        if let Some(temp) = config.temperature {
            body["temperature"] = json!(temp);
        }

        if !tools.is_empty() {
            body["tools"] = json!(Self::serialize_tools(tools));
            body["tool_choice"] = json!("auto");
        }

        body
    }

    async fn parse_sse_stream(
        &self,
        response: reqwest::Response,
        app: &tauri::AppHandle,
        run_id: &str,
        iteration: u32,
    ) -> Result<LlmResponse, String> {
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut text = String::new();
        let mut usage = Usage::default();
        let mut finish_reason: Option<String> = None;
        let mut tool_calls: BTreeMap<u64, PartialToolCall> = BTreeMap::new();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| format!("stream error: {}", e))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

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
                    let event: Value = match serde_json::from_str(data) {
                        Ok(value) => value,
                        Err(e) => {
                            debug!("failed to parse Vercel SSE data: {} - {}", e, data);
                            continue;
                        }
                    };
                    Self::apply_stream_event(
                        &event,
                        &mut text,
                        &mut tool_calls,
                        &mut finish_reason,
                        &mut usage,
                        app,
                        run_id,
                        iteration,
                    );
                }
            }
        }

        let mut content = Vec::new();
        if !text.is_empty() {
            content.push(ContentBlock::Text { text });
        }
        content.extend(Self::finalize_tool_calls(
            tool_calls, app, run_id, iteration,
        ));

        let stop_reason = Self::map_finish_reason(finish_reason.as_deref(), &content);
        Ok(LlmResponse {
            content,
            stop_reason,
            usage,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_stream_event(
        event: &Value,
        text: &mut String,
        tool_calls: &mut BTreeMap<u64, PartialToolCall>,
        finish_reason: &mut Option<String>,
        usage: &mut Usage,
        app: &tauri::AppHandle,
        run_id: &str,
        iteration: u32,
    ) {
        let (text_chunks, tool_input_deltas) =
            Self::accumulate_stream_event(event, text, tool_calls, finish_reason, usage);

        for chunk in text_chunks {
            emit_agent_llm_chunk(app, run_id, &chunk, iteration);
            emit_log_chunk(app, run_id, vec![("stdout".to_string(), chunk)]);
        }

        for delta in tool_input_deltas {
            emit_agent_content_block(
                app,
                run_id,
                iteration,
                "tool_input_delta",
                json!({
                    "type": "tool_input_delta",
                    "id": delta.id,
                    "name": delta.name,
                    "partial_json": delta.partial_json,
                }),
            );
        }
    }

    fn accumulate_stream_event(
        event: &Value,
        text: &mut String,
        tool_calls: &mut BTreeMap<u64, PartialToolCall>,
        finish_reason: &mut Option<String>,
        usage: &mut Usage,
    ) -> (Vec<String>, Vec<ToolInputDelta>) {
        let mut text_chunks = Vec::new();
        let mut tool_input_deltas = Vec::new();

        Self::parse_usage(event.get("usage"), usage);

        for choice in event["choices"].as_array().into_iter().flatten() {
            if let Some(reason) = choice["finish_reason"].as_str() {
                *finish_reason = Some(reason.to_string());
            }

            let delta = &choice["delta"];
            if let Some(chunk) = delta["content"].as_str() {
                if !chunk.is_empty() {
                    text.push_str(chunk);
                    text_chunks.push(chunk.to_string());
                }
            }

            for tool_call in delta["tool_calls"].as_array().into_iter().flatten() {
                let index = tool_call["index"].as_u64().unwrap_or(0);
                let entry = tool_calls.entry(index).or_default();
                if let Some(id) = tool_call["id"].as_str() {
                    entry.id = id.to_string();
                }
                if let Some(name) = tool_call["function"]["name"].as_str() {
                    entry.name = name.to_string();
                }
                if let Some(arguments) = tool_call["function"]["arguments"].as_str() {
                    entry.arguments.push_str(arguments);
                    tool_input_deltas.push(ToolInputDelta {
                        id: entry.id.clone(),
                        name: entry.name.clone(),
                        partial_json: arguments.to_string(),
                    });
                }
            }
        }

        (text_chunks, tool_input_deltas)
    }

    fn finalize_tool_calls(
        tool_calls: BTreeMap<u64, PartialToolCall>,
        app: &tauri::AppHandle,
        run_id: &str,
        iteration: u32,
    ) -> Vec<ContentBlock> {
        tool_calls
            .into_values()
            .filter(|call| !call.name.is_empty())
            .map(|call| {
                let input = serde_json::from_str::<Value>(&call.arguments).unwrap_or_else(|e| {
                    debug!(
                        tool = %call.name,
                        json_len = call.arguments.len(),
                        "failed to parse Vercel tool input JSON: {}",
                        e
                    );
                    json!({})
                });
                let id = if call.id.is_empty() {
                    format!("call_{}", call.name)
                } else {
                    call.id
                };
                emit_agent_content_block(
                    app,
                    run_id,
                    iteration,
                    "tool_use",
                    json!({
                        "type": "tool_use",
                        "id": id,
                        "name": call.name,
                        "input": input,
                    }),
                );
                ContentBlock::ToolUse {
                    id,
                    name: call.name,
                    input,
                }
            })
            .collect()
    }

    fn parse_complete_response(value: Value) -> Result<LlmResponse, String> {
        let mut usage = Usage::default();
        Self::parse_usage(value.get("usage"), &mut usage);

        let choice = value["choices"]
            .as_array()
            .and_then(|choices| choices.first())
            .cloned()
            .ok_or_else(|| "Vercel response had no choices".to_string())?;
        let message = &choice["message"];
        let mut content = Vec::new();

        if let Some(text) = message["content"].as_str() {
            if !text.is_empty() {
                content.push(ContentBlock::Text {
                    text: text.to_string(),
                });
            }
        }

        let mut tool_calls = BTreeMap::new();
        for (index, tool_call) in message["tool_calls"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let arguments = tool_call["function"]["arguments"]
                .as_str()
                .unwrap_or("{}")
                .to_string();
            tool_calls.insert(
                index as u64,
                PartialToolCall {
                    id: tool_call["id"].as_str().unwrap_or_default().to_string(),
                    name: tool_call["function"]["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    arguments,
                },
            );
        }
        content.extend(
            tool_calls
                .into_values()
                .filter(|call| !call.name.is_empty())
                .map(|call| ContentBlock::ToolUse {
                    id: call.id,
                    name: call.name,
                    input: serde_json::from_str::<Value>(&call.arguments).unwrap_or(json!({})),
                }),
        );

        let finish_reason = choice["finish_reason"].as_str();
        Ok(LlmResponse {
            stop_reason: Self::map_finish_reason(finish_reason, &content),
            content,
            usage,
        })
    }

    fn parse_usage(value: Option<&Value>, usage: &mut Usage) {
        let Some(value) = value else {
            return;
        };
        if let Some(input) = value["prompt_tokens"]
            .as_u64()
            .or_else(|| value["input_tokens"].as_u64())
        {
            usage.input_tokens = input as u32;
        }
        if let Some(output) = value["completion_tokens"]
            .as_u64()
            .or_else(|| value["output_tokens"].as_u64())
        {
            usage.output_tokens = output as u32;
        }
    }

    fn map_finish_reason(reason: Option<&str>, content: &[ContentBlock]) -> StopReason {
        match reason {
            Some("tool_calls") => StopReason::ToolUse,
            Some("length") => StopReason::MaxTokens,
            _ if content
                .iter()
                .any(|block| matches!(block, ContentBlock::ToolUse { .. })) =>
            {
                StopReason::ToolUse
            }
            _ => StopReason::EndTurn,
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for VercelProvider {
    fn name(&self) -> &str {
        "vercel"
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
        let mut body = Self::request_body(config, messages, tools);
        body["stream"] = json!(true);
        body["stream_options"] = json!({ "include_usage": true });

        let response = self
            .client
            .post(VERCEL_CHAT_COMPLETIONS_URL)
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Vercel request failed: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            let error_json: Value =
                serde_json::from_str(&error_body).unwrap_or(json!({"error": error_body}));
            let msg = error_json["error"]["message"]
                .as_str()
                .unwrap_or(&error_body);
            return Err(format!("Vercel API error ({}): {}", status, msg));
        }

        self.parse_sse_stream(response, app, run_id, iteration)
            .await
    }

    async fn chat_complete(
        &self,
        config: &LlmConfig,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<LlmResponse, String> {
        let body = Self::request_body(config, messages, tools);

        let response = self
            .client
            .post(VERCEL_CHAT_COMPLETIONS_URL)
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Vercel request failed: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            let error_json: Value =
                serde_json::from_str(&error_body).unwrap_or(json!({"error": error_body}));
            let msg = error_json["error"]["message"]
                .as_str()
                .unwrap_or(&error_body);
            return Err(format!("Vercel API error ({}): {}", status, msg));
        }

        let value = response
            .json::<Value>()
            .await
            .map_err(|e| format!("Vercel response parse failed: {}", e))?;
        Self::parse_complete_response(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::llm_provider::{ChatMessage, ContentBlock, LlmConfig, ToolDefinition};

    #[test]
    fn filters_and_sorts_tool_capable_language_models() {
        let value = json!({
            "object": "list",
            "data": [
                {
                    "id": "z-provider/no-tools",
                    "name": "No Tools",
                    "owned_by": "z-provider",
                    "type": "language",
                    "tags": [],
                    "context_window": 8192
                },
                {
                    "id": "openai/gpt-5.4",
                    "name": "GPT-5.4",
                    "owned_by": "openai",
                    "type": "language",
                    "tags": ["tool-use", "vision"],
                    "context_window": 200000
                },
                {
                    "id": "anthropic/claude-sonnet-4.6",
                    "name": "Claude Sonnet 4.6",
                    "owned_by": "anthropic",
                    "type": "language",
                    "tags": ["tool-use"],
                    "context_window": 1000000
                },
                {
                    "id": "openai/embed",
                    "name": "Embedding",
                    "owned_by": "openai",
                    "type": "embedding",
                    "tags": ["tool-use"]
                }
            ]
        });

        let options = parse_model_options(value).expect("models should parse");

        assert_eq!(
            options,
            vec![
                VercelGatewayModelOption {
                    label: "Anthropic - Claude Sonnet 4.6".to_string(),
                    value: "anthropic/claude-sonnet-4.6".to_string(),
                },
                VercelGatewayModelOption {
                    label: "OpenAI - GPT-5.4".to_string(),
                    value: "openai/gpt-5.4".to_string(),
                },
            ]
        );
        assert_eq!(cached_model_context_window("openai/gpt-5.4"), Some(200000));
        assert!(cached_model_supports_images("openai/gpt-5.4"));
    }

    #[test]
    fn serializes_openai_compatible_messages_and_tools() {
        let config = LlmConfig {
            model: "openai/gpt-5.4".to_string(),
            max_tokens: 1000,
            temperature: Some(0.3),
            system_prompt: "Be useful.".to_string(),
        };
        let messages = vec![
            ChatMessage {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: "Check status".to_string(),
                }],
                created_at: None,
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: vec![ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "status".to_string(),
                    input: json!({ "id": 7 }),
                }],
                created_at: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: "ok".to_string(),
                    is_error: false,
                }],
                created_at: None,
            },
        ];
        let tools = vec![ToolDefinition {
            name: "status".to_string(),
            description: "Fetch status".to_string(),
            input_schema: json!({ "type": "object" }),
        }];

        let body = VercelProvider::request_body(&config, &messages, &tools);

        assert_eq!(body["model"], "openai/gpt-5.4");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(
            body["messages"][2]["tool_calls"][0]["function"]["name"],
            "status"
        );
        assert_eq!(body["messages"][3]["role"], "tool");
        assert_eq!(body["tools"][0]["function"]["name"], "status");
    }

    #[test]
    fn parses_complete_tool_call_response() {
        let value = json!({
            "choices": [
                {
                    "finish_reason": "tool_calls",
                    "message": {
                        "role": "assistant",
                        "content": "",
                        "tool_calls": [
                            {
                                "id": "call_123",
                                "type": "function",
                                "function": {
                                    "name": "read_file",
                                    "arguments": "{\"path\":\"README.md\"}"
                                }
                            }
                        ]
                    }
                }
            ],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5
            }
        });

        let response = VercelProvider::parse_complete_response(value).expect("response parses");

        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert_eq!(response.usage.input_tokens, 10);
        assert_eq!(response.usage.output_tokens, 5);
        assert!(matches!(
            &response.content[0],
            ContentBlock::ToolUse { name, input, .. }
                if name == "read_file" && input["path"] == "README.md"
        ));
    }

    #[test]
    fn assembles_streaming_tool_call_arguments() {
        let mut text = String::new();
        let mut tool_calls = BTreeMap::new();
        let mut finish_reason = None;
        let mut usage = Usage::default();

        let first = json!({
            "choices": [
                {
                    "delta": {
                        "content": "Reading",
                        "tool_calls": [
                            {
                                "index": 0,
                                "id": "call_123",
                                "type": "function",
                                "function": {
                                    "name": "read_file",
                                    "arguments": "{\"path\""
                                }
                            }
                        ]
                    },
                    "finish_reason": null
                }
            ]
        });
        let second = json!({
            "choices": [
                {
                    "delta": {
                        "tool_calls": [
                            {
                                "index": 0,
                                "function": {
                                    "arguments": ":\"README.md\"}"
                                }
                            }
                        ]
                    },
                    "finish_reason": "tool_calls"
                }
            ],
            "usage": {
                "prompt_tokens": 11,
                "completion_tokens": 7
            }
        });

        let (text_chunks, input_deltas) = VercelProvider::accumulate_stream_event(
            &first,
            &mut text,
            &mut tool_calls,
            &mut finish_reason,
            &mut usage,
        );
        assert_eq!(text_chunks, vec!["Reading".to_string()]);
        assert_eq!(input_deltas.len(), 1);

        VercelProvider::accumulate_stream_event(
            &second,
            &mut text,
            &mut tool_calls,
            &mut finish_reason,
            &mut usage,
        );

        let call = tool_calls.get(&0).expect("tool call should be assembled");
        assert_eq!(text, "Reading");
        assert_eq!(call.id, "call_123");
        assert_eq!(call.name, "read_file");
        assert_eq!(call.arguments, "{\"path\":\"README.md\"}");
        assert_eq!(finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(usage.input_tokens, 11);
        assert_eq!(usage.output_tokens, 7);
    }
}
