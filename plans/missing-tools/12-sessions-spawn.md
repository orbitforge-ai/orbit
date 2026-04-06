# Plan: `sessions_spawn` Tool

## Context
OpenClaw's `Sessions (sessions_spawn)` offers more flexible session spawning than `spawn_sub_agents`. It supports different runtime modes (subagent vs. ACP), one-shot ("run") vs. persistent ("session") modes, and isolated sessions. Orbit's `spawn_sub_agents` is limited to parallel one-shot tasks that can't spawn further agents.

## What It Does
Spawn an isolated session with configurable runtime and mode. `mode="run"` is one-shot (runs to completion), `mode="session"` stays alive for ongoing interaction via `session_send`. Unlike `spawn_sub_agents`, this creates a single session with more control.

## Backend Changes

### New file: `src-tauri/src/executor/tools/sessions_spawn.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod sessions_spawn;` and `Box::new(sessions_spawn::SessionsSpawnTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):
```rust
ToolDefinition {
    name: "sessions_spawn".to_string(),
    description: "Spawn an isolated session with configurable mode. mode='run' executes once and returns results. mode='session' creates a persistent session you can interact with via session_send.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "goal": {
                "type": "string",
                "description": "The initial goal/message for the spawned session"
            },
            "mode": {
                "type": "string",
                "enum": ["run", "session"],
                "description": "'run' = one-shot execution (default). 'session' = persistent, interact via session_send."
            },
            "agent": {
                "type": "string",
                "description": "Target agent name/ID. Default: spawn on self."
            },
            "label": {
                "type": "string",
                "description": "Optional label for easy reference"
            },
            "timeout_seconds": {
                "type": "integer",
                "description": "Timeout for 'run' mode (default: 300, max: 600)"
            },
            "allow_sub_agents": {
                "type": "boolean",
                "description": "Whether spawned session can spawn further sub-agents. Default: false."
            }
        },
        "required": ["goal"]
    }),
}
```

**Execution** (in `execute()` method):
1. Resolve target agent (self or specified)
2. Create chat session with appropriate type
3. If `mode="run"`: execute agent session, wait for completion, return result (similar to `spawn_sub_agents` but single)
4. If `mode="session"`: create session, trigger first run, return session_id immediately for later interaction
5. Track `allow_sub_agents` flag (currently `spawn_sub_agents` hard-blocks nesting)

### `src-tauri/src/executor/permissions.rs`
```rust
"sessions_spawn" => (RiskLevel::AutoAllow, String::new()),
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`
```ts
import { PlusCircle } from 'lucide-react';
sessions_spawn: { Icon: PlusCircle, colorClass: 'text-emerald-400' },
```

### `src/screens/AgentInspector/ConfigTab.tsx`
Add to "Sessions" category:
```ts
{ id: 'sessions_spawn', label: 'Spawn Session' },
```

## Permission Level
- **AutoAllow** — same as `spawn_sub_agents`

## Dependencies
- Existing session creation and agent execution infrastructure
- Benefits from `session_send` (plan 09) for persistent mode interaction

## Key Design Decisions
- `mode="session"` is the key differentiator from `spawn_sub_agents` — creates a persistent session
- `allow_sub_agents` relaxes the current hard nesting restriction when explicitly opted in
- Chain depth still tracked and enforced
- Single session (vs. `spawn_sub_agents` which creates multiple)

## Verification
1. `sessions_spawn { goal: "Count to 10", mode: "run" }` → confirm executes and returns result
2. `sessions_spawn { goal: "Be ready for questions", mode: "session" }` → confirm returns session_id
3. Use `session_send` to interact with persistent session → confirm conversation continues
4. Test `allow_sub_agents: true` → confirm spawned session can spawn further
5. Test timeout enforcement in run mode
