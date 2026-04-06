# Plan: Expand `shell_command` — Background Execution + Process Management

> **Reconciled**: Merges OpenClaw's `Exec` + `Process` tools and Claude Code's `Bash` `run_in_background` pattern into a single expansion of Orbit's existing `shell_command` tool. No new tool names — just new parameters on `shell_command`.

## Context
Both OpenClaw and Claude Code support background command execution. Claude Code's approach is simpler and more elegant: add a `run_in_background` boolean to the existing Bash tool, which returns a `background_id` + output file path. Process management (poll, kill) is handled via the same tool or simple follow-up calls. This avoids creating separate `exec` and `process` tools.

## What Changes

### Expand `shell_command` schema with new parameters:

```rust
"run_in_background": {
    "type": "boolean",
    "description": "Run command in background, return immediately with a process_id. Output written to a file you can read later. Default: false."
},
"process_action": {
    "type": "string",
    "enum": ["list", "poll", "kill"],
    "description": "Manage background processes. 'list' = show active, 'poll' = get output for process_id, 'kill' = terminate process_id. Only used without 'command'."
},
"process_id": {
    "type": "string",
    "description": "Background process ID (for poll/kill actions)"
}
```

### How it works:

**Background execution** (replaces plan 03):
```
shell_command { command: "npm run dev", run_in_background: true }
→ { "process_id": "abc123", "output_path": "~/.orbit/agents/{id}/bg/abc123.log", "status": "running" }
```

**Process management** (replaces plan 04):
```
shell_command { process_action: "list" }
→ [{ "process_id": "abc123", "command": "npm run dev", "running": true, "started_at": "..." }]

shell_command { process_action: "poll", process_id: "abc123" }
→ { "output": "...", "running": true }

shell_command { process_action: "kill", process_id: "abc123" }
→ "Process killed."
```

## Backend Changes

### New module: `src-tauri/src/executor/bg_processes.rs`
Same `BgProcessRegistry` as original plan 03, but accessed through `shell_command` instead of a separate tool:
- `register()`, `list()`, `get_output()`, `kill()`, `is_running()`
- Output written to `~/.orbit/agents/{agent_id}/bg/{process_id}.log`
- Rolling buffer cap at 1MB
- Auto-cleanup of terminated processes after 5 minutes

### Modify existing: `src-tauri/src/executor/tools/shell_command.rs`

> After plan 00 refactor, `shell_command` lives in its own file. Expand the existing `ShellCommandTool` — update `definition()` to add new schema fields, update `execute()` to handle background mode and process actions.

Modify `shell_command` execute method:
```rust
"shell_command" => {
    // Check if this is a process management action (no command)
    if let Some(action) = input["process_action"].as_str() {
        let registry = app.state::<BgProcessRegistry>();
        return match action {
            "list" => { /* list processes for this agent */ }
            "poll" => { /* get output for process_id */ }
            "kill" => { /* terminate process_id */ }
            _ => Err(format!("unknown process_action: {}", action)),
        };
    }
    
    let command = input["command"].as_str().ok_or("missing 'command'")?;
    let background = input["run_in_background"].as_bool().unwrap_or(false);
    
    if background {
        // Spawn process, register in BgProcessRegistry
        // Return process_id + output_path immediately
    } else {
        // Existing blocking execution (unchanged)
    }
}
```

### `src-tauri/src/executor/permissions.rs`
No changes needed — shell_command classification already covers this. Process management actions inherit the same classification based on the command that was originally run.

Add classification for process_action:
```rust
if let Some(action) = input["process_action"].as_str() {
    return match action {
        "list" | "poll" => (RiskLevel::AutoAllow, String::new()),
        "kill" => (RiskLevel::Prompt, "Kill background process".to_string()),
        _ => (RiskLevel::Prompt, format!("Process action: {}", action)),
    };
}
```

## Frontend Changes

### `src/components/chat/ToolUseBlock.tsx`
Build this on top of the shared tool presentation system from plan [26-tool-use-ui](26-tool-use-ui.md).

This plan should add the `shell_command`-specific formatter/data mapping needed for:
- background-process cards
- live/stopped indicators
- human-readable shell/process result formatting

## Why This Approach
- **Claude Code pattern**: One tool, multiple modes — simpler mental model for the LLM
- **No new tool names**: Reduces tool definition bloat
- **Backward compatible**: Existing `shell_command` calls work unchanged
- **OpenClaw's separate tools** were over-engineered for Orbit's use case

## Verification
1. `shell_command { command: "sleep 30", run_in_background: true }` → immediate return with process_id
2. `shell_command { process_action: "list" }` → shows running process
3. `shell_command { process_action: "poll", process_id: "..." }` → shows output
4. `shell_command { process_action: "kill", process_id: "..." }` → terminates
5. Regular `shell_command { command: "echo hi" }` → unchanged blocking behavior
