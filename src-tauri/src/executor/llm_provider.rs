use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::db::DbPool;
use crate::executor::agent_tools::ToolExecutionContext;
use crate::executor::anthropic::AnthropicProvider;
use crate::executor::claude_cli::{ClaudeCliProvider, SessionFn};
use crate::executor::mcp_server::{McpServerHandle, McpSession};
use crate::executor::minimax::MiniMaxProvider;
use crate::executor::permissions::PermissionRegistry;

/// A provider name routes through a local CLI binary rather than an HTTP API.
/// Central list so call sites don't duplicate the literal names.
pub const CLI_PROVIDERS: &[&str] = &["claude-cli", "codex-cli"];

pub fn is_cli_provider(name: &str) -> bool {
    CLI_PROVIDERS.contains(&name)
}

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

    /// Send a non-streaming chat completion.
    async fn chat_complete(
        &self,
        config: &LlmConfig,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<LlmResponse, String>;
}

// ─── Model context window lookup ────────────────────────────────────────────

/// Returns the maximum context window size (in tokens) for a given provider/model pair.
pub fn model_context_window(provider_name: &str, model: &str) -> u32 {
    match provider_name {
        // Claude CLI routes through the same Anthropic models, so the window
        // is governed by the model, not the transport.
        "anthropic" | "claude-cli" => match model {
            "claude-opus-4-7" | "claude-opus-4-6" | "claude-sonnet-4-6" => 1_000_000,
            "claude-haiku-4-5-20251001"
            | "claude-opus-4-20250415"
            | "claude-opus-4-20250514"
            | "claude-sonnet-4-20250514"
            | "claude-3-5-sonnet-20241022"
            | "claude-3-5-haiku-20241022" => 200_000,
            other => {
                warn!(
                    "Unknown anthropic/claude-cli model '{}' — falling back to 200k context window",
                    other
                );
                200_000
            }
        },
        "codex-cli" => 200_000,
        "minimax" => match model {
            "MiniMax-M2.7" | "MiniMax-M2.7-highspeed" => 204_800,
            "MiniMax-M2.5" | "MiniMax-M2.5-highspeed" => 204_800,
            "MiniMax-M2.1" | "MiniMax-M2.1-highspeed" => 204_800,
            "MiniMax-M2" => 204_800,
            other => {
                warn!(
                    "Unknown minimax model '{}' — falling back to 200k context window",
                    other
                );
                200_000
            }
        },
        other => {
            warn!(
                "Unknown provider '{}' for model '{}' — falling back to 200k context window",
                other, model
            );
            200_000
        }
    }
}

/// Returns whether the configured provider/model is expected to support image inputs.
pub fn model_supports_images(provider_name: &str, model: &str) -> bool {
    match provider_name {
        "anthropic" => matches!(
            model,
            "claude-opus-4-7"
                | "claude-opus-4-6"
                | "claude-sonnet-4-6"
                | "claude-opus-4-20250415"
                | "claude-sonnet-4-20250514"
                | "claude-haiku-4-5-20251001"
                | "claude-opus-4-20250514"
                | "claude-3-5-sonnet-20241022"
                | "claude-3-5-haiku-20241022"
        ),
        // Claude CLI uses the same models but image support over the MCP
        // bridge is not yet verified end to end — keep disabled for v1.
        "minimax" => matches!(
            model,
            "MiniMax-M2.7"
                | "MiniMax-M2.7-highspeed"
                | "MiniMax-M2.5"
                | "MiniMax-M2.5-highspeed"
                | "MiniMax-M2.1"
                | "MiniMax-M2.1-highspeed"
                | "MiniMax-M2"
        ),
        // CLI providers: image support is disabled in v1 until end-to-end
        // verification through the MCP bridge is completed.
        "claude-cli" | "codex-cli" => false,
        _ => false,
    }
}

pub fn extract_text_response(response: &LlmResponse) -> Result<String, String> {
    let text = response
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim()
        .to_string();

    if !text.is_empty() {
        return Ok(text);
    }

    if response
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolUse { .. }))
    {
        return Err("model returned a tool request instead of analysis text".to_string());
    }

    Err("model returned no text analysis".to_string())
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
        // CLI providers: the bare factory returns a tool-less provider. Agent
        // call sites that need the Orbit tool catalog exposed to the CLI should
        // use `create_provider_with_mcp` (or construct the CLI provider
        // directly) so they can attach an MCP handle + session factory.
        "claude-cli" => Ok(Box::new(ClaudeCliProvider::new(None))),
        "codex-cli" => Err(
            "codex-cli provider is not yet implemented. Select another provider for now.".to_string(),
        ),
        other => Err(format!("unsupported LLM provider: {}", other)),
    }
}

/// Per-call wiring data used to construct an `McpSession` lazily each time a
/// CLI provider makes a streaming call. Owned by the agent-loop caller.
#[derive(Clone)]
pub struct AgentMcpWiring {
    pub handle: McpServerHandle,
    pub agent_id: String,
    pub run_id: String,
    pub tool_ctx: Arc<ToolExecutionContext>,
    pub tools: Vec<ToolDefinition>,
    pub permission_registry: PermissionRegistry,
    pub app: tauri::AppHandle,
    pub db: DbPool,
}

impl AgentMcpWiring {
    fn into_session_fn(self) -> (McpServerHandle, SessionFn) {
        let handle = self.handle.clone();
        let wiring = self;
        let f: SessionFn = Arc::new(move || {
            Some(McpSession {
                run_id: wiring.run_id.clone(),
                agent_id: wiring.agent_id.clone(),
                tool_ctx: wiring.tool_ctx.clone(),
                tools: wiring.tools.clone(),
                permission_registry: wiring.permission_registry.clone(),
                app: wiring.app.clone(),
                db: wiring.db.clone(),
            })
        });
        (handle, f)
    }
}

/// Factory variant that wires an MCP bridge handle and a per-run session
/// factory into CLI providers so the CLI's inner agent loop can call Orbit
/// tools. Non-CLI providers ignore the wiring and behave identically to
/// `create_provider`.
pub fn create_provider_with_mcp(
    provider_name: &str,
    api_key: String,
    wiring: Option<AgentMcpWiring>,
) -> Result<Box<dyn LlmProvider>, String> {
    match provider_name {
        "anthropic" => Ok(Box::new(AnthropicProvider::new(api_key))),
        "minimax" => Ok(Box::new(MiniMaxProvider::new(api_key))),
        "claude-cli" => {
            let provider = match wiring {
                Some(w) => {
                    let (handle, session_fn) = w.into_session_fn();
                    ClaudeCliProvider::new(Some(handle)).with_session_fn(session_fn)
                }
                None => ClaudeCliProvider::new(None),
            };
            Ok(Box::new(provider))
        }
        "codex-cli" => Err(
            "codex-cli provider is not yet implemented. Select another provider for now.".to_string(),
        ),
        other => Err(format!("unsupported LLM provider: {}", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        extract_text_response, model_context_window, model_supports_images, ContentBlock,
        LlmResponse, StopReason, Usage,
    };

    #[test]
    fn known_models_have_expected_context_windows() {
        assert_eq!(
            model_context_window("anthropic", "claude-opus-4-7"),
            1_000_000
        );
        assert_eq!(
            model_context_window("anthropic", "claude-opus-4-6"),
            1_000_000
        );
        assert_eq!(
            model_context_window("anthropic", "claude-sonnet-4-6"),
            1_000_000
        );
        assert_eq!(
            model_context_window("anthropic", "claude-haiku-4-5-20251001"),
            200_000
        );
        assert_eq!(model_context_window("minimax", "MiniMax-M2.7"), 204_800);
        assert_eq!(model_context_window("minimax", "MiniMax-M2.5"), 204_800);
    }

    #[test]
    fn unknown_models_fall_back_conservatively() {
        assert_eq!(
            model_context_window("unknown-provider", "unknown-model"),
            200_000
        );
    }

    #[test]
    fn image_support_checks_known_models() {
        assert!(model_supports_images("anthropic", "claude-sonnet-4-6"));
        assert!(model_supports_images("minimax", "MiniMax-M2.7"));
        assert!(!model_supports_images("anthropic", "unknown-model"));
    }

    #[test]
    fn extracts_text_response_from_blocks() {
        let response = LlmResponse {
            content: vec![ContentBlock::Text {
                text: "Image analysis".to_string(),
            }],
            stop_reason: StopReason::EndTurn,
            usage: Usage::default(),
        };
        assert_eq!(
            extract_text_response(&response).expect("text should be extracted"),
            "Image analysis"
        );
    }
}
