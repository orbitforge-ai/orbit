# Plan: `sessions_list` Tool

## Context
OpenClaw's `Sessions (sessions_list)` tool lets agents list sessions with filters. Orbit has `list_chat_sessions` as a Tauri command (frontend-facing), but agents have no tool to discover or enumerate their own sessions. This enables agents to find prior conversations, track sub-agent sessions, or audit their own activity.

## What It Does
List sessions for the current agent with optional filters: session type, execution state, date range, and search text. Returns session metadata with last message preview.

## Backend Changes

### New file: `src-tauri/src/executor/tools/sessions_list.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod sessions_list;` and `Box::new(sessions_list::SessionsListTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):
```rust
ToolDefinition {
    name: "sessions_list".to_string(),
    description: "List sessions for this agent with optional filters. Returns session IDs, titles, types, states, and last message previews.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "session_type": {
                "type": "string",
                "enum": ["user_chat", "sub_agent", "bus_message"],
                "description": "Filter by session type"
            },
            "state": {
                "type": "string",
                "enum": ["running", "success", "failure", "cancelled"],
                "description": "Filter by execution state"
            },
            "limit": {
                "type": "integer",
                "description": "Maximum sessions to return (default: 20, max: 100)"
            },
            "search": {
                "type": "string",
                "description": "Search session titles"
            }
        }
    }),
}
```

**Execution** (in `execute()` method):
```rust
"sessions_list" => {
    let db = ctx.db.as_ref().ok_or("sessions_list: no database")?;
    let limit = input["limit"].as_u64().unwrap_or(20).min(100) as i64;
    let session_type = input["session_type"].as_str();
    let state = input["state"].as_str();
    let search = input["search"].as_str();
    
    // Query sessions for this agent with filters
    let sessions = db::list_agent_sessions(
        db, &ctx.agent_id, session_type, state, search, limit
    ).await?;
    
    let formatted: Vec<_> = sessions.iter().map(|s| json!({
        "id": s.id,
        "title": s.title,
        "type": s.session_type,
        "state": s.execution_state,
        "created_at": s.created_at,
        "last_message_preview": s.last_message_preview,
    })).collect();
    
    Ok((serde_json::to_string_pretty(&formatted).unwrap(), false))
}
```

### `src-tauri/src/db/`
May need a new query function `list_agent_sessions()` with filter parameters, or extend existing `list_chat_sessions`.

### `src-tauri/src/executor/permissions.rs`
```rust
"sessions_list" => (RiskLevel::AutoAllow, String::new()),
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`
```ts
import { List } from 'lucide-react';
sessions_list: { Icon: List, colorClass: 'text-muted' },
```

### `src/screens/AgentInspector/ConfigTab.tsx`
Add to "Sessions" category:
```ts
{ id: 'sessions_list', label: 'List Sessions' },
```

## Permission Level
- **AutoAllow** — read-only, agent-scoped

## Dependencies
- Existing database session models
- May need extended query function with filter support

## Verification
1. `sessions_list {}` → confirm current agent's sessions listed
2. Filter by `session_type: "sub_agent"` → only sub-agent sessions
3. Filter by `state: "success"` → only completed sessions
4. Verify `limit` works
5. Verify search filters by title
