# Plan: `session_status` Tool

## Context
OpenClaw's `Session Status` tool shows a status card with usage, timing, and cost information. Orbit agents currently can't introspect their own resource usage. This is useful for agents managing budgets, monitoring performance, or reporting on their own activity.

## What It Does
Show a session status card including: execution state, total tokens used, estimated cost, duration, iteration count, and model info. Can query the current session or any session by ID.

## Backend Changes

### New file: `src-tauri/src/executor/tools/session_status.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod session_status;` and `Box::new(session_status::SessionStatusTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):
```rust
ToolDefinition {
    name: "session_status".to_string(),
    description: "Show session status: execution state, token usage, estimated cost, duration, and model info. Use for monitoring resource usage or reporting on activity.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "session_id": {
                "type": "string",
                "description": "Session ID to check. Use 'current' for this session. Default: 'current'."
            }
        }
    }),
}
```

**Execution** (in `execute()` method):
```rust
"session_status" => {
    let db = ctx.db.as_ref().ok_or("session_status: no database")?;
    let session_id = input["session_id"].as_str().unwrap_or("current");
    let resolved_id = if session_id == "current" {
        ctx.current_session_id.as_deref().ok_or("no current session")?
    } else {
        session_id
    };
    
    // Verify ownership
    let session = db::get_chat_session(db, resolved_id).await?;
    if session.agent_id != ctx.agent_id {
        return Err("Cannot access other agents' sessions".to_string());
    }
    
    // Gather stats from session and runs
    let runs = db::list_runs_for_session(db, resolved_id).await?;
    let total_tokens: i64 = runs.iter().map(|r| r.total_tokens.unwrap_or(0)).sum();
    let total_iterations: i64 = runs.iter().map(|r| r.iterations.unwrap_or(0)).sum();
    let duration = compute_session_duration(&runs);
    let cost = estimate_cost(total_tokens, &session.model);
    
    let status = json!({
        "session_id": resolved_id,
        "state": session.execution_state,
        "title": session.title,
        "type": session.session_type,
        "total_tokens": total_tokens,
        "estimated_cost_usd": cost,
        "total_iterations": total_iterations,
        "duration_seconds": duration,
        "model": session.model,
        "created_at": session.created_at,
    });
    
    Ok((serde_json::to_string_pretty(&status).unwrap(), false))
}
```

### `src-tauri/src/executor/permissions.rs`
```rust
"session_status" => (RiskLevel::AutoAllow, String::new()),
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`
```ts
import { BarChart3 } from 'lucide-react';
session_status: { Icon: BarChart3, colorClass: 'text-muted' },
```

### `src/screens/AgentInspector/ConfigTab.tsx`
Add to "Sessions" category:
```ts
{ id: 'session_status', label: 'Session Status' },
```

## Permission Level
- **AutoAllow** — read-only introspection, agent-scoped

## Dependencies
- Existing database session and run models
- May need new DB query: `list_runs_for_session(session_id)` if not already available
- Cost estimation helper (simple token-based calculation by model)

## Verification
1. Mid-session: `session_status {}` → confirm current session stats returned
2. Check token count is reasonable
3. Query a completed session by ID → confirm historical stats work
4. Query another agent's session → confirm access denied
