# Plan: `session_history` Tool

## Context
OpenClaw's `Session History` tool lets agents fetch message history for any session. Orbit agents currently have no way to inspect past conversations or other sessions' history. This is useful for context gathering, summarization, and multi-session coordination.

## What It Does
Fetch message history for a session by ID. Returns messages with role, content, and timestamps. Supports pagination via `limit` and `offset`. Agent can only access sessions belonging to the same agent (security isolation).

## Backend Changes

### New file: `src-tauri/src/executor/tools/session_history.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod session_history;` and `Box::new(session_history::SessionHistoryTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):
```rust
ToolDefinition {
    name: "session_history".to_string(),
    description: "Fetch message history for a session. Returns messages with roles and timestamps. Useful for reviewing past conversations or gathering context from other sessions.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "session_id": {
                "type": "string",
                "description": "The session ID to fetch history for. Use 'current' for the current session."
            },
            "limit": {
                "type": "integer",
                "description": "Maximum messages to return (default: 50, max: 200)"
            },
            "offset": {
                "type": "integer",
                "description": "Skip this many messages from the start (for pagination)"
            }
        },
        "required": ["session_id"]
    }),
}
```

**Execution** (in `execute()` method):
```rust
"session_history" => {
    let db = ctx.db.as_ref().ok_or("session_history: no database")?;
    let session_id = input["session_id"].as_str().ok_or("session_history: missing session_id")?;
    
    // Resolve "current" to actual session ID
    let resolved_id = if session_id == "current" {
        ctx.current_session_id.as_deref().ok_or("session_history: no current session")?
    } else {
        session_id
    };
    
    // Verify session belongs to this agent (security)
    let session = db::get_chat_session(db, resolved_id).await
        .map_err(|e| format!("session not found: {}", e))?;
    if session.agent_id != ctx.agent_id {
        return Err("session_history: cannot access sessions from other agents".to_string());
    }
    
    let limit = input["limit"].as_u64().unwrap_or(50).min(200) as i64;
    let offset = input["offset"].as_u64().unwrap_or(0) as i64;
    
    let messages = db::list_chat_messages(db, resolved_id, limit, offset).await
        .map_err(|e| format!("failed to fetch messages: {}", e))?;
    
    // Format for agent consumption (strip internal fields)
    let formatted: Vec<_> = messages.iter().map(|m| json!({
        "role": m.role,
        "content": m.content,  // summarized/truncated if large
        "created_at": m.created_at,
    })).collect();
    
    Ok((serde_json::to_string_pretty(&formatted).unwrap(), false))
}
```

### `src-tauri/src/executor/permissions.rs`
```rust
"session_history" => (RiskLevel::AutoAllow, String::new()),
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`
```ts
import { History } from 'lucide-react';
session_history: { Icon: History, colorClass: 'text-muted' },
```

### `src/screens/AgentInspector/ConfigTab.tsx`
Add new "Sessions" category:
```ts
{
    label: 'Sessions',
    tools: [
        { id: 'session_history', label: 'Session History' },
    ],
},
```

## Permission Level
- **AutoAllow** — read-only, agent-scoped (can only see own sessions)

## Dependencies
- Existing database queries in `src-tauri/src/db/` — may need a `list_chat_messages` function if not already exposed
- Existing `ChatMessage` model

## Verification
1. Start a session, send some messages, then call `session_history { session_id: "current" }` → confirm messages returned
2. Create a second session, try to access it by ID → confirm it works (same agent)
3. Try to access another agent's session → confirm access denied
4. Test pagination with `limit` and `offset`
5. Confirm `"current"` resolves correctly
