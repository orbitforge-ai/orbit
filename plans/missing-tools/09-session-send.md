# Plan: `session_send` Tool

## Context
Orbit's `send_message` creates a **new** session on the target agent. OpenClaw's `Session Send` sends a message into an **existing** session, resuming a prior conversation. This is crucial for multi-turn coordination — an agent can continue a conversation rather than starting fresh each time.

## What It Does
Send a message into an existing session by ID, optionally triggering the agent to process it. Unlike `send_message` which always creates new sessions, this appends to an existing conversation thread.

## Backend Changes

### New file: `src-tauri/src/executor/tools/session_send.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod session_send;` and `Box::new(session_send::SessionSendTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):
```rust
ToolDefinition {
    name: "session_send".to_string(),
    description: "Send a message into an existing session. Unlike send_message (which creates new sessions), this appends to an existing conversation. Use sessionKey or session_id to identify the target.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "session_id": {
                "type": "string",
                "description": "The target session ID"
            },
            "message": {
                "type": "string",
                "description": "The message to send"
            },
            "trigger_run": {
                "type": "boolean",
                "description": "If true, trigger the agent to process the message (start a new run). Default: true."
            },
            "wait_for_result": {
                "type": "boolean",
                "description": "If true and trigger_run is true, wait for the agent to finish and return its response. Default: false."
            }
        },
        "required": ["session_id", "message"]
    }),
}
```

**Execution** (in `execute()` method):
1. Verify session exists and belongs to an accessible agent (same agent or known target)
2. Insert message as a `user` role message into the session
3. If `trigger_run: true` (default), submit a RunRequest to the executor
4. If `wait_for_result: true`, poll until the run reaches terminal state and return finish_summary
5. Return confirmation with session_id and optional result

### `src-tauri/src/executor/permissions.rs`
```rust
"session_send" => {
    let sid = input["session_id"].as_str().unwrap_or("<unknown>");
    (RiskLevel::Prompt, format!("Send to existing session '{}'", truncate_for_display(sid, 20)))
}
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`
```ts
import { CornerDownRight } from 'lucide-react';
session_send: { Icon: CornerDownRight, colorClass: 'text-blue-400' },
```

### `src/screens/AgentInspector/ConfigTab.tsx`
Add to "Sessions" category:
```ts
{ id: 'session_send', label: 'Session Send' },
```

## Permission Level
- **Prompt** — writes into a session and potentially triggers agent execution

## Dependencies
- Existing database session/message models
- Existing executor `RunRequest` submission
- Implements after plan 08 (session_history) since they share the Sessions category

## Key Design Decisions
- Security: agents can only send to sessions that belong to them or to agents they've previously communicated with via `send_message`
- Chain depth tracking: increments chain_depth to prevent infinite loops
- `trigger_run` defaults to true (most useful behavior)

## Verification
1. Create a session via `send_message`, note the session_id
2. Use `session_send { session_id: "...", message: "Follow up question" }` → confirm message appended
3. With `trigger_run: true` → confirm agent processes the message
4. With `wait_for_result: true` → confirm response returned
5. Try sending to non-existent session → error
6. Try sending to another agent's session without prior relationship → access denied
