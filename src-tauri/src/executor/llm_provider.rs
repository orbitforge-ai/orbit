use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::executor::anthropic::AnthropicProvider;
use crate::executor::minimax::MiniMaxProvider;

// ─── Provider-agnostic types ─────────────────────────────────────────────────

/// Configuration for an LLM call.
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub model: String,
    pub max_tokens: u32,
    pub temperature: Option<f64>,
    pub system_prompt: String,
}

/// A single message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String, // "user" | "assistant"
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub created_at: Option<String>,
}

/// A content block within a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    Image {
        media_type: String,
        data: String,
    },
}

/// A tool definition exposed to the LLM.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// The assembled response from an LLM call.
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: Vec<ContentBlock>,
    pub stop_reason: StopReason,
    pub usage: Usage,
}

/// Why the LLM stopped generating.
#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
}

/// Token usage for a single call.
#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

// ─── Provider trait ──────────────────────────────────────────────────────────

/// Trait that every LLM provider implements.
/// The agent loop calls this trait — never a provider directly.
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    /// Human-readable name for logging (e.g. "anthropic", "openai").
    #[allow(dead_code)]
    fn name(&self) -> &str;

    /// Send a streaming chat completion.
    /// Text deltas are emitted to the frontend via the provided AppHandle.
    /// The full assembled response is returned.
    async fn chat_streaming(
        &self,
        config: &LlmConfig,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        app: &tauri::AppHandle,
        run_id: &str,
        iteration: u32,
    ) -> Result<LlmResponse, String>;
}

// ─── Model context window lookup ────────────────────────────────────────────

/// Returns the maximum context window size (in tokens) for a given model.
pub fn model_context_window(model: &str) -> u32 {
    match model {
        // Anthropic models
        "claude-sonnet-4-20250514"
        | "claude-opus-4-20250514"
        | "claude-haiku-4-5-20251001"
        | "claude-3-5-sonnet-20241022"
        | "claude-3-5-haiku-20241022" => 200_000,
        // MiniMax models
        "MiniMax-M2.7" | "MiniMax-M2.7-highspeed" => 1_000_000,
        "MiniMax-M2.5" | "MiniMax-M2.5-highspeed" => 1_000_000,
        "MiniMax-M2.1" | "MiniMax-M2.1-highspeed" => 1_000_000,
        "MiniMax-M2" => 1_000_000,
        other => {
            warn!(
                "Unknown model '{}' — falling back to 200k context window",
                other
            );
            200_000
        }
    }
}

// ─── Provider factory ────────────────────────────────────────────────────────

/// Create a provider instance by name.
/// Future providers: add a match arm and a new module.
pub fn create_provider(
    provider_name: &str,
    api_key: String,
) -> Result<Box<dyn LlmProvider>, String> {
    match provider_name {
        "anthropic" => Ok(Box::new(AnthropicProvider::new(api_key))),
        "minimax" => Ok(Box::new(MiniMaxProvider::new(api_key))),
        other => Err(format!("unsupported LLM provider: {}", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::model_context_window;

    #[test]
    fn known_models_have_expected_context_windows() {
        assert_eq!(model_context_window("claude-opus-4-20250514"), 200_000);
        assert_eq!(model_context_window("MiniMax-M2.7"), 1_000_000);
    }

    #[test]
    fn unknown_models_fall_back_conservatively() {
        assert_eq!(model_context_window("unknown-model"), 200_000);
    }
}
