# Plan: `subagents` Management Tool

## Context
Orbit's `spawn_sub_agents` launches parallel sub-agents but provides no way to manage them after spawning. OpenClaw's `Subagents` tool lets agents list, kill, or steer running sub-agents. This is critical for orchestration — an agent needs to monitor progress, cancel stuck tasks, or redirect sub-agents mid-flight.

## What It Does
Manage spawned sub-agents for the current session. Actions: `list` (show status of all sub-agents), `kill` (terminate a specific sub-agent), `steer` (send new instructions to a running sub-agent).

## Backend Changes

### New file: `src-tauri/src/executor/tools/subagents.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod subagents;` and `Box::new(subagents::SubagentsTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):
```rust
ToolDefinition {
    name: "subagents".to_string(),
    description: "List, kill, or steer spawned sub-agents. Use 'list' to see status, 'kill' to terminate, 'steer' to send new instructions to a running sub-agent.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["list", "kill", "steer"],
                "description": "Action to perform"
            },
            "session_id": {
                "type": "string",
                "description": "Sub-agent session ID (required for kill/steer)"
            },
            "message": {
                "type": "string",
                "description": "New instructions for the sub-agent (required for steer)"
            }
        },
        "required": ["action"]
    }),
}
```

**Execution** (in `execute()` method):
```rust
"subagents" => {
    let db = ctx.db.as_ref().ok_or("subagents: no database")?;
    let action = input["action"].as_str().ok_or("subagents: missing action")?;
    
    match action {
        "list" => {
            // Query all sub_agent sessions spawned by current session
            let sessions = db::list_child_sessions(
                db, ctx.current_session_id.as_deref().unwrap_or("")
            ).await?;
            let formatted: Vec<_> = sessions.iter().map(|s| json!({
                "session_id": s.id,
                "title": s.title,
                "state": s.execution_state,
                "finish_summary": s.finish_summary,
                "created_at": s.created_at,
            })).collect();
            Ok((serde_json::to_string_pretty(&formatted).unwrap(), false))
        }
        "kill" => {
            let sid = input["session_id"].as_str()
                .ok_or("subagents: kill requires session_id")?;
            // Cancel via SessionExecutionRegistry
            let registry = ctx.session_registry.as_ref()
                .ok_or("subagents: no session registry")?;
            registry.cancel(sid).await;
            Ok((format!("Sub-agent session '{}' cancelled.", sid), false))
        }
        "steer" => {
            let sid = input["session_id"].as_str()
                .ok_or("subagents: steer requires session_id")?;
            let message = input["message"].as_str()
                .ok_or("subagents: steer requires message")?;
            // Insert a user message into the sub-agent session
            db::insert_chat_message(db, sid, "user", message).await?;
            Ok((format!("Steering message sent to sub-agent '{}'.", sid), false))
        }
        _ => Err(format!("subagents: unknown action '{}'", action)),
    }
}
```

### New DB query needed
`list_child_sessions(parent_session_id)` — query sessions where `parent_session_id` matches.

### `src-tauri/src/executor/permissions.rs`
```rust
"subagents" => {
    let action = input["action"].as_str().unwrap_or("list");
    match action {
        "list" => (RiskLevel::AutoAllow, String::new()),
        "kill" => (RiskLevel::Prompt, "Kill sub-agent".to_string()),
        "steer" => (RiskLevel::AutoAllow, String::new()),
        _ => (RiskLevel::Prompt, format!("Subagent action: {}", action)),
    }
}
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`
```ts
import { GitFork } from 'lucide-react';
subagents: { Icon: GitFork, colorClass: 'text-emerald-400' },
```

### `src/screens/AgentInspector/ConfigTab.tsx`
Add to "Agent Control" category:
```ts
{ id: 'subagents', label: 'Manage Sub-Agents' },
```

## Permission Level
- `list`, `steer`: **AutoAllow** — read/coordination only
- `kill`: **Prompt** — terminates running work

## Dependencies
- Existing `SessionExecutionRegistry` for cancellation
- Existing DB session models
- `parent_session_id` field on `ChatSession` model (verify it exists)

## Verification
1. Spawn sub-agents with `spawn_sub_agents`, then `subagents { action: "list" }` → see all spawned sessions with states
2. `subagents { action: "kill", session_id: "..." }` → confirm sub-agent cancelled
3. `subagents { action: "steer", session_id: "...", message: "Change approach to X" }` → confirm new message injected
4. Test with no sub-agents → empty list
5. Confirm kill permission prompt appears
