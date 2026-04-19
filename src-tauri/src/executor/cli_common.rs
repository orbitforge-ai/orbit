//! Shared helpers for CLI-backed LLM providers (Claude CLI, Codex CLI).
//!
//! Responsibilities:
//!  - locate a CLI binary on PATH (with PATH fallbacks typical on macOS)
//!  - serialize Orbit chat history into the CLI's stream-json input shape
//!  - parse a JSONL event stream back into Orbit's `LlmResponse`
//!  - emit intermediate events to the UI so users can see tool calls while
//!    the CLI runs its internal agent loop
use std::path::PathBuf;

use crate::executor::llm_provider::{ChatMessage, ContentBlock};

/// Resolve a CLI binary on PATH. Falls back to common Homebrew / local-bin
/// locations that Tauri GUI apps sometimes miss when PATH is inherited from
/// launchd instead of the shell.
pub fn resolve_cli(binary: &str) -> Option<PathBuf> {
    if let Ok(path) = which::which(binary) {
        return Some(path);
    }
    for candidate in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
    ] {
        let p = PathBuf::from(candidate).join(binary);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Render a chat transcript as plain text for v1 multi-turn support. The
/// current turn is the last `user` message; everything before it is prepended
/// as an inline transcript the model can see but not edit. This is coarser
/// than the CLIs' native session resume but does not depend on any session
/// state being persisted between turns.
pub fn transcript_for_cli(messages: &[ChatMessage]) -> (String, String) {
    // Find the index of the last user text message — that's the current turn.
    let mut current_turn = String::new();
    let mut history_end_idx: usize = messages.len();

    for (i, msg) in messages.iter().enumerate().rev() {
        if msg.role == "user" {
            if let Some(text) = extract_text(&msg.content) {
                current_turn = text;
                history_end_idx = i;
                break;
            }
        }
    }

    if current_turn.is_empty() {
        // Fallback: no user text message found — concatenate everything.
        let combined = messages
            .iter()
            .filter_map(|m| extract_text(&m.content).map(|t| format!("{}: {}", m.role, t)))
            .collect::<Vec<_>>()
            .join("\n\n");
        return (String::new(), combined);
    }

    let history_text = messages[..history_end_idx]
        .iter()
        .filter_map(|m| {
            extract_text(&m.content)
                .map(|t| format!("[{}]\n{}", m.role, t))
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    (history_text, current_turn)
}

fn extract_text(blocks: &[ContentBlock]) -> Option<String> {
    let mut out = String::new();
    for block in blocks {
        match block {
            ContentBlock::Text { text } => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(text);
            }
            ContentBlock::ToolResult { content, .. } => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&format!("[tool_result]\n{}", content));
            }
            _ => {}
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::llm_provider::ContentBlock;

    #[test]
    fn transcript_separates_history_from_current_turn() {
        let messages = vec![
            ChatMessage {
                role: "user".into(),
                content: vec![ContentBlock::Text {
                    text: "first question".into(),
                }],
                created_at: None,
            },
            ChatMessage {
                role: "assistant".into(),
                content: vec![ContentBlock::Text {
                    text: "first answer".into(),
                }],
                created_at: None,
            },
            ChatMessage {
                role: "user".into(),
                content: vec![ContentBlock::Text {
                    text: "second question".into(),
                }],
                created_at: None,
            },
        ];
        let (history, current) = transcript_for_cli(&messages);
        assert_eq!(current, "second question");
        assert!(history.contains("first question"));
        assert!(history.contains("first answer"));
    }

    #[test]
    fn transcript_with_only_current_turn_has_empty_history() {
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: vec![ContentBlock::Text {
                text: "hello".into(),
            }],
            created_at: None,
        }];
        let (history, current) = transcript_for_cli(&messages);
        assert_eq!(current, "hello");
        assert!(history.is_empty());
    }
}
