# Plan: `schedule` Tool — Task Schedules + Pulse Management

## Context

Orbit already has a mature scheduling backend for product tasks plus a dedicated pulse workflow:

- generic schedules are implemented through `tasks` + `schedules`
- pulse is implemented as a special `agent_loop` task tagged with `"pulse"` plus a recurring schedule and a dedicated `pulse` chat session

Today that functionality is only available through the product UI and Tauri commands. Agents cannot inspect or manage recurring automation for themselves, which is a meaningful gap for agentic workflows. A scheduling tool should expose both generic schedule control and first-class pulse management through one agent-facing interface.

## What It Does

Adds a unified `schedule` tool with two domains:

- **task schedules** for existing product tasks owned by the current agent
- **pulse** configuration for the current agent's recurring self-run prompt

This is intentionally one tool rather than separate `schedule` and `pulse` tools because pulse is implemented on top of the same scheduler primitives and should feel like a specialized schedule mode, not a totally separate system.

## Backend Changes

### New file: `src-tauri/src/executor/tools/schedule.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod schedule;` and `Box::new(schedule::ScheduleTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):

```rust
ToolDefinition {
    name: "schedule".to_string(),
    description: "Inspect and manage recurring automation for this agent. Supports task schedules and the agent's pulse configuration.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": [
                    "list",
                    "create",
                    "update",
                    "toggle",
                    "delete",
                    "preview",
                    "pulse_get",
                    "pulse_set",
                    "pulse_run"
                ],
                "description": "Scheduling action to perform"
            },
            "schedule_id": {
                "type": "string",
                "description": "Schedule ID for update/toggle/delete"
            },
            "task_id": {
                "type": "string",
                "description": "Existing task ID owned by the current agent"
            },
            "enabled": {
                "type": "boolean",
                "description": "Enable or disable a schedule"
            },
            "kind": {
                "type": "string",
                "enum": ["recurring", "one_shot"],
                "description": "Schedule kind. Default: recurring."
            },
            "config": {
                "type": "object",
                "description": "Schedule config payload matching Orbit's existing schedule config schema"
            },
            "preview_count": {
                "type": "integer",
                "description": "How many future runs to preview (default: 5, max: 20)"
            },
            "pulse_content": {
                "type": "string",
                "description": "Prompt content for the agent's recurring pulse"
            },
            "pulse_enabled": {
                "type": "boolean",
                "description": "Whether pulse scheduling should be active"
            },
            "pulse_schedule": {
                "type": "object",
                "description": "Recurring schedule config for pulse"
            }
        },
        "required": ["action"]
    }),
}
```

### Behavior

**Task schedule actions**

- `list`
  - Return schedules for tasks owned by `ctx.agent_id`
  - Exclude pulse-backed schedules from the generic list to avoid duplication
  - Include `schedule_id`, `task_id`, `task_name`, `kind`, `enabled`, `next_run_at`, `last_run_at`

- `create`
  - Require `task_id` and `config`
  - Verify the referenced task belongs to the current agent
  - Reuse the existing schedule creation logic and schedule config schema
  - Return the created schedule metadata

- `update`
  - Require `schedule_id`
  - Allow updating `config` and optionally `enabled`
  - Verify the schedule belongs to a task owned by the current agent
  - Recompute `next_run_at` from the updated config

- `toggle`
  - Require `schedule_id` and `enabled`
  - Verify ownership through the underlying task
  - Reuse the existing enable/disable path

- `delete`
  - Require `schedule_id`
  - Verify ownership through the underlying task
  - Delete the schedule

- `preview`
  - Use `config` directly without creating anything
  - Return the next N run times using the existing preview helper

**Pulse actions**

- `pulse_get`
  - Return the current agent's pulse config exactly once, including `enabled`, `content`, `schedule`, `task_id`, `schedule_id`, `session_id`, `next_run_at`, `last_run_at`

- `pulse_set`
  - Require `pulse_content`, `pulse_schedule`, and `pulse_enabled`
  - Reuse the current pulse update flow so the tool updates:
    - `pulse.md`
    - the pulse backing task
    - the pulse schedule
    - the pulse chat session if needed

- `pulse_run`
  - Trigger the existing pulse backing task immediately
  - Return the task/session identifiers and a “triggered” status

### Reuse Existing Backend Instead of Re-implementing It

This tool should wrap the existing scheduler/pulse command-layer logic rather than duplicating it:

- generic scheduling should reuse the existing schedule CRUD and preview logic
- pulse actions should reuse the current pulse configuration/update behavior

If command reuse is awkward, extract shared helpers out of `commands/schedules.rs` and `commands/pulse.rs` into a scheduler service module and have both the commands and the new tool call that shared layer.

### Security / Scope

- Agents may only manage schedules for tasks whose `agent_id == ctx.agent_id`
- Agents may only manage their own pulse
- The tool should never expose or mutate schedules belonging to other agents
- Pulse remains a special agent-local capability and should not accept a foreign `agent_id`

### `src-tauri/src/executor/permissions.rs`

```rust
"schedule" => {
    let action = input["action"].as_str().unwrap_or("list");
    match action {
        "list" | "preview" | "pulse_get" => (RiskLevel::AutoAllow, String::new()),
        "create" | "update" | "toggle" | "delete" | "pulse_set" | "pulse_run" => {
            (RiskLevel::Prompt, format!("Schedule action: {}", action))
        }
        _ => (RiskLevel::Prompt, format!("Schedule action: {}", action)),
    }
}
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`

```ts
import { Clock3 } from 'lucide-react';
schedule: { Icon: Clock3, colorClass: 'text-warning' },
```

### `src/screens/AgentInspector/ConfigTab.tsx`

Add a new "Scheduling" category:

```ts
{
    label: 'Scheduling',
    tools: [{ id: 'schedule', label: 'Schedules & Pulse' }],
},
```

### Optional `ToolUseBlock` polish

Build this on top of the shared tool presentation foundation from plan [26-tool-use-ui](26-tool-use-ui.md). This plan should add the schedule-specific formatter needed to render:
- `list` results as a compact schedule table
- `preview` results as a short ordered list of timestamps
- `pulse_get` / `pulse_set` results with pulse status, next run, and pulse session link when available

## Permission Level

- `list`, `preview`, `pulse_get`: **AutoAllow**
- `create`, `update`, `toggle`, `delete`, `pulse_set`, `pulse_run`: **Prompt**

Rationale: reads are safe, but writes change future autonomous behavior and should be explicitly approved.

## Dependencies

- Existing `tasks`, `schedules`, and `pulse` backend models/commands
- Existing schedule config types (`RecurringConfig`, `CreateSchedule`, etc.)
- Existing scheduler preview logic
- No new DB tables required

## Key Design Decisions

- **One tool, two domains**: generic scheduling and pulse belong together because pulse is scheduler-backed
- **Pulse is first-class**: do not force the agent to manipulate pulse indirectly by editing tagged tasks and schedules manually
- **Ownership enforcement**: all generic schedule actions must verify the underlying task belongs to the current agent
- **Do not expose global scheduler administration**: this is agent-scoped automation control, not a system admin tool
- **Preview without side effects**: agents can reason about recurrence timing before saving changes

## Verification

1. `schedule { action: "list" }` → returns schedules for tasks owned by the current agent, excluding pulse
2. `schedule { action: "preview", config: {...}, preview_count: 5 }` → returns 5 future run times without creating anything
3. `schedule { action: "create", task_id: "...", kind: "recurring", config: {...} }` → creates a schedule for an agent-owned task
4. `schedule { action: "toggle", schedule_id: "...", enabled: false }` → disables the schedule
5. `schedule { action: "delete", schedule_id: "..." }` → removes the schedule
6. `schedule { action: "pulse_get" }` → returns current pulse content, schedule, and linked session/task IDs
7. `schedule { action: "pulse_set", pulse_content: "...", pulse_schedule: {...}, pulse_enabled: true }` → updates pulse config and next run
8. `schedule { action: "pulse_run" }` → triggers the pulse task immediately
9. Attempt to mutate another agent's task schedule → access denied
10. Confirm permission prompts appear for mutating actions
