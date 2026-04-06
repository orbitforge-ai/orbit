# Plan: `message` Tool (External Channels)

## Context
Orbit's `send_message` tool communicates between agents via the internal Agent Bus. OpenClaw's `Message` tool sends messages to external channels — Slack, Discord, email, webhooks. This lets agents notify humans, post updates, or integrate with external communication systems.

## What It Does
Send messages to configured external channels. Supports multiple channel types: Slack (webhook), Discord (webhook), email (SMTP), and generic webhooks. Channels are configured per-agent or globally.

## Backend Changes

### New module: `src-tauri/src/executor/channels.rs`
```rust
pub enum ChannelType {
    Slack,
    Discord,
    Webhook,
    Email,
}

pub struct ChannelConfig {
    pub id: String,
    pub name: String,
    pub channel_type: ChannelType,
    pub webhook_url: Option<String>,  // Slack/Discord/webhook
    pub smtp_config: Option<SmtpConfig>,  // Email
    pub enabled: bool,
}

pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password_key: String,  // keychain reference
    pub from_address: String,
    pub to_addresses: Vec<String>,
}

pub async fn send_to_channel(config: &ChannelConfig, message: &str, subject: Option<&str>) -> Result<String, String> {
    match config.channel_type {
        ChannelType::Slack => send_slack(config, message).await,
        ChannelType::Discord => send_discord(config, message).await,
        ChannelType::Webhook => send_webhook(config, message).await,
        ChannelType::Email => send_email(config, message, subject).await,
    }
}
```

### New file: `src-tauri/src/executor/tools/message.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod message;` and `Box::new(message::MessageTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):
```rust
ToolDefinition {
    name: "message".to_string(),
    description: "Send a message to a configured external channel (Slack, Discord, email, webhook). Use 'list' to see available channels.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["send", "list"],
                "description": "Action to perform. 'list' shows available channels, 'send' sends a message."
            },
            "channel": {
                "type": "string",
                "description": "Channel name or ID to send to (required for 'send')"
            },
            "text": {
                "type": "string",
                "description": "The message text to send"
            },
            "subject": {
                "type": "string",
                "description": "Optional subject line (used for email channels)"
            }
        },
        "required": ["action"]
    }),
}
```

### Channel config storage
Store channel configs in the agent's workspace config (`AgentWorkspaceConfig`) or in a dedicated `channels.json` in the agent root directory.

### `src-tauri/src/executor/permissions.rs`
```rust
"message" => {
    let action = input["action"].as_str().unwrap_or("send");
    match action {
        "list" => (RiskLevel::AutoAllow, String::new()),
        "send" => {
            let channel = input["channel"].as_str().unwrap_or("<unknown>");
            (RiskLevel::PromptDangerous, format!("Send external message to channel '{}'", channel))
        }
        _ => (RiskLevel::Prompt, "Message action".to_string()),
    }
}
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`
```ts
import { MessageSquare } from 'lucide-react';
message: { Icon: MessageSquare, colorClass: 'text-blue-400' },
```

### `src/screens/AgentInspector/ConfigTab.tsx`
Add to "Communication" category:
```ts
{ id: 'message', label: 'External Messages' },
```

### New UI: Channel Configuration
Add a "Channels" section in the Agent Inspector config tab where users can:
- Add/remove channels (Slack webhook URL, Discord webhook URL, email SMTP settings)
- Test channel connectivity
- Enable/disable individual channels

## Permission Level
- `list`: **AutoAllow**
- `send`: **PromptDangerous** — sends messages visible to external humans, irreversible

## Dependencies
- `reqwest` for webhook POST requests (already available)
- `lettre` crate for SMTP email (optional, can defer email support)

## Key Design Decisions
- Slack/Discord via incoming webhooks (no OAuth needed, simple setup)
- Email is optional/phase-2 (requires SMTP config, `lettre` dependency)
- Messages are always Prompt-gated to prevent spam
- Channel configs stored per-agent for isolation

## Verification
1. Configure a Slack webhook channel → `message { action: "send", channel: "slack-test", text: "Hello from Orbit!" }` → confirm message appears in Slack
2. `message { action: "list" }` → confirm channels listed
3. Test with unconfigured channel → clear error
4. Confirm PromptDangerous permission prompt appears before sending
5. Test Discord webhook similarly
