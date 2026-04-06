# Plan: Expand `gateway` → Agent Config & Self-Management

> **Reconciled**: Merges OpenClaw's `Gateway` tool with Claude Code's `Config` tool. Claude Code's Config supports get/set of runtime settings with scoped precedence. Combined with OpenClaw's restart/reload concept.

## Context
OpenClaw has `Gateway` (restart, inspect config, apply config, hot-reload). Claude Code has `Config` (get/set settings with global/project scoping). Both serve the same purpose: agent self-management. For Orbit, this becomes a `config` tool that lets agents inspect and modify their own workspace configuration at runtime.

## What It Does
Self-management tool for agents. Actions: `get` (read config value), `set` (update config value), `list` (show all settings), `info` (agent metadata). Replaces the old `gateway` name with the clearer `config` name (following Claude Code's convention).

## Backend Changes

### New file: `src-tauri/src/executor/tools/config.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod config;` and `Box::new(config::ConfigTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):
```rust
ToolDefinition {
    name: "config".to_string(),
    description: "Get or set your agent configuration. Use 'get' to read a setting, 'set' to change one, 'list' to see all settings, 'info' for agent metadata.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["get", "set", "list", "info"],
                "description": "Action to perform"
            },
            "setting": {
                "type": "string",
                "description": "Setting name (for get/set). e.g., 'model', 'temperature', 'maxIterations'"
            },
            "value": {
                "description": "New value (for set action)"
            }
        },
        "required": ["action"]
    }),
}
```

**Modifiable settings** (whitelist):
- `model`, `temperature`, `maxIterations`, `maxTotalTokens`
- `webSearchProvider`, `memoryEnabled`

**Blocked settings** (security):
- `permissionMode`, `permissionRules`, `allowedTools`, `disabledSkills`

**Info action returns:**
```json
{
    "agent_id": "...",
    "workspace": "~/.orbit/agents/{id}/workspace",
    "chain_depth": 0,
    "is_sub_agent": false,
    "session_id": "...",
    "run_id": "..."
}
```

### `src-tauri/src/executor/permissions.rs`
```rust
"config" => {
    let action = input["action"].as_str().unwrap_or("list");
    match action {
        "get" | "list" | "info" => (RiskLevel::AutoAllow, String::new()),
        "set" => {
            let setting = input["setting"].as_str().unwrap_or("<unknown>");
            (RiskLevel::Prompt, format!("Update config: '{}'", setting))
        }
        _ => (RiskLevel::Prompt, "Config action".to_string()),
    }
}
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`
```ts
import { Settings } from 'lucide-react';
config: { Icon: Settings, colorClass: 'text-muted' },
```

### `src/screens/AgentInspector/ConfigTab.tsx`
Add to "Agent Control" category:
```ts
{ id: 'config', label: 'Self-Config' },
```

## Permission Level
- `get`, `list`, `info`: **AutoAllow**
- `set`: **Prompt** (modifies agent behavior)

## Verification
1. `config { action: "list" }` → show all settings
2. `config { action: "get", setting: "model" }` → return current model
3. `config { action: "set", setting: "temperature", value: 0.5 }` → update + confirm
4. `config { action: "set", setting: "permissionMode" }` → BLOCKED
5. `config { action: "info" }` → show agent metadata