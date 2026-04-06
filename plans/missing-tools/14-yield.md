# Plan: `yield_turn` Tool

## Context
OpenClaw's `Yield` tool lets an agent end its current turn and wait for sub-agent results or external input. Orbit agents currently run in a continuous loop until they call `finish`. There's no way to pause mid-execution and resume when async work completes. This is essential for the "spawn then wait" pattern.

## What It Does
End the agent's current LLM turn. The agent loop pauses and waits for a trigger to resume — either sub-agent completion, a timeout, or an explicit message. The next LLM call receives the accumulated results.

## Backend Changes

### New file: `src-tauri/src/executor/tools/yield_turn.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod yield_turn;` and `Box::new(yield_turn::YieldTurnTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):
```rust
ToolDefinition {
    name: "yield_turn".to_string(),
    description: "End your current turn and wait. Use after spawning sub-agents or sessions to receive their results in your next turn. Specify what you're waiting for.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "reason": {
                "type": "string",
                "description": "What you're waiting for (e.g., 'sub-agent results', 'user input')"
            },
            "timeout_seconds": {
                "type": "integer",
                "description": "Maximum time to wait before resuming (default: 300, max: 600)"
            },
            "wait_for": {
                "type": "string",
                "enum": ["sub_agents", "message", "timeout"],
                "description": "'sub_agents' = resume when all spawned sub-agents complete. 'message' = resume on next message. 'timeout' = resume after timeout. Default: 'sub_agents'."
            }
        }
    }),
}
```

**Execution** (in `execute()` method):
The key insight is that `yield_turn` needs to signal the agent loop to pause. This can be done by returning a special flag.

```rust
"yield_turn" => {
    let reason = input["reason"].as_str().unwrap_or("Waiting for results");
    let timeout = input["timeout_seconds"].as_u64().unwrap_or(300).min(600);
    let wait_for = input["wait_for"].as_str().unwrap_or("sub_agents");
    
    // Return result with is_finish=false but set a "yield" flag
    // The agent loop checks for this and enters a wait state
    Ok((json!({
        "action": "yield",
        "reason": reason,
        "wait_for": wait_for,
        "timeout_seconds": timeout,
    }).to_string(), false))
}
```

### `src-tauri/src/executor/session_agent.rs`
Modify the agent loop to detect yield results:

```rust
// After execute_tool returns, check for yield
if tool_name == "yield_turn" {
    // Enter yield state:
    // 1. Save current messages/state
    // 2. If wait_for == "sub_agents": poll child sessions until all terminal
    // 3. If wait_for == "message": wait for new message in session
    // 4. If wait_for == "timeout": sleep for specified duration
    // 5. Collect results and inject as next user message
    // 6. Continue agent loop
    
    let yield_results = wait_for_yield_condition(wait_for, timeout, db, session_id).await;
    // Inject results as tool_result content and continue loop
}
```

### `src-tauri/src/executor/permissions.rs`
```rust
"yield_turn" => (RiskLevel::AutoAllow, String::new()),
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`
```ts
import { PauseCircle } from 'lucide-react';
yield_turn: { Icon: PauseCircle, colorClass: 'text-warning' },
```

### `src/components/chat/ToolUseBlock.tsx`
The base implementation still needs a visible waiting state so the run is understandable while paused. Build that state on top of the shared tool presentation foundation from plan [26-tool-use-ui](26-tool-use-ui.md), and add the yield-specific waiting/timer presentation within that shared system.

### `src/screens/AgentInspector/ConfigTab.tsx`
Add to "Agent Control" category:
```ts
{ id: 'yield_turn', label: 'Yield Turn' },
```

## Permission Level
- **AutoAllow** — no side effects, just pauses execution

## Dependencies
- Agent loop modification in `session_agent.rs`
- Child session polling (similar to existing `spawn_sub_agents` polling logic)

## Key Design Decisions
- `yield_turn` does NOT end the session — it pauses the LLM loop temporarily
- The agent loop already polls for cancellation; yield is a similar "wait" pattern
- Results from the wait period are injected as the tool_result, so the LLM sees them naturally
- This is distinct from `finish` (which ends the session permanently)

## Verification
1. Spawn sub-agents → `yield_turn { wait_for: "sub_agents" }` → confirm agent pauses until sub-agents complete, then receives their results
2. `yield_turn { wait_for: "timeout", timeout_seconds: 5 }` → confirm agent pauses 5s then resumes
3. Confirm yield shows "Waiting..." indicator in UI
4. Test timeout enforcement (doesn't hang forever)
5. Confirm agent loop continues normally after yield completes
