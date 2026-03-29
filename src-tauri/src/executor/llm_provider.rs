use serde::{ Deserialize, Serialize };

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
    iteration: u32
  ) -> Result<LlmResponse, String>;
}

// ─── Provider factory ────────────────────────────────────────────────────────

/// Create a provider instance by name.
/// Future providers: add a match arm and a new module.
pub fn create_provider(
  provider_name: &str,
  api_key: String
) -> Result<Box<dyn LlmProvider>, String> {
  match provider_name {
    "anthropic" => Ok(Box::new(AnthropicProvider::new(api_key))),
    "minimax" => Ok(Box::new(MiniMaxProvider::new(api_key))),
    other => Err(format!("unsupported LLM provider: {}", other)),
  }
}
