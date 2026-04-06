# Plan: Task Management Tools

> **Source**: Claude Code's `TaskCreate`, `TaskList`, `TaskGet`, `TaskUpdate`, `TaskStop`, `TaskOutput`, `TodoWrite` tools. Not present in OpenClaw.
> **Approach**: New tool — Orbit's agents have no task tracking. Implement as a single `task` tool with actions (like `shell_command` pattern) rather than 7 separate tools.

## Context

Claude Code has a rich task management system with 7 separate tools. Agents use it to break down work, track progress, manage dependencies between tasks, and coordinate with sub-agents. Orbit agents currently have no way to track their own work items. This is a significant gap for complex, multi-step agent workflows.

Orbit already has a separate scheduled-automation concept named `Task` in the product UI and database. That overlap is acceptable for now, but this plan should keep the new agent-planning data clearly scoped as agent/session-local state to avoid coupling it to the scheduler feature.

## What It Does

A unified `task` tool with actions: `create`, `list`, `get`, `update`, `delete`. Tasks have subjects, descriptions, statuses (pending/in_progress/completed), and can block each other. Stored per-session in the database.

The tool name remains `task`, but the backing data model stays separate from scheduled tasks by using a dedicated `agent_tasks` table and agent-focused UI copy.

## Backend Changes

### Database: `src-tauri/src/db/`

New table `agent_tasks`:

```sql
CREATE TABLE IF NOT EXISTS agent_tasks (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    subject TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'pending',  -- pending, in_progress, completed
    active_form TEXT,  -- present tense description for UI
    blocked_by TEXT,  -- JSON array of task IDs
    metadata TEXT,  -- JSON object for arbitrary data
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (session_id) REFERENCES chat_sessions(id)
);
```

### New file: `src-tauri/src/executor/tools/task.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod task;` and `Box::new(task::TaskTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):

```rust
ToolDefinition {
    name: "task".to_string(),
    description: "Track work items. Create tasks to break down work, update status as you progress, manage dependencies. Actions: create, list, get, update, delete.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["create", "list", "get", "update", "delete"],
                "description": "Action to perform"
            },
            "subject": {
                "type": "string",
                "description": "Task subject/title (for create)"
            },
            "description": {
                "type": "string",
                "description": "Task description (for create/update)"
            },
            "task_id": {
                "type": "string",
                "description": "Task ID (for get/update/delete)"
            },
            "status": {
                "type": "string",
                "enum": ["pending", "in_progress", "completed"],
                "description": "Task status (for update)"
            },
            "active_form": {
                "type": "string",
                "description": "Present tense form shown in UI (e.g., 'Running tests')"
            },
            "blocked_by": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Task IDs that must complete before this one"
            }
        },
        "required": ["action"]
    }),
}
```

**Execution** (in `execute()` method):

```rust
"task" => {
    let db = ctx.db.as_ref().ok_or("task: no database")?;
    let action = input["action"].as_str().ok_or("task: missing action")?;
    let session_id = ctx.current_session_id.as_deref().unwrap_or("");

    match action {
        "create" => {
            let subject = input["subject"].as_str().ok_or("task: create requires subject")?;
            let id = uuid::Uuid::new_v4().to_string();
            // Insert into agent_tasks table
            Ok((json!({"task_id": id, "status": "pending"}).to_string(), false))
        }
        "list" => {
            // Query all tasks for current session
            let tasks = db::list_agent_tasks(db, session_id).await?;
            Ok((serde_json::to_string_pretty(&tasks).unwrap(), false))
        }
        "get" => {
            let task_id = input["task_id"].as_str().ok_or("task: get requires task_id")?;
            let task = db::get_agent_task(db, task_id).await?;
            Ok((serde_json::to_string_pretty(&task).unwrap(), false))
        }
        "update" => {
            let task_id = input["task_id"].as_str().ok_or("task: update requires task_id")?;
            // Apply status, description, active_form, blocked_by updates
            Ok(("Task updated.".to_string(), false))
        }
        "delete" => {
            let task_id = input["task_id"].as_str().ok_or("task: delete requires task_id")?;
            db::delete_agent_task(db, task_id).await?;
            Ok(("Task deleted.".to_string(), false))
        }
        _ => Err(format!("task: unknown action '{}'", action)),
    }
}
```

### `src-tauri/src/executor/permissions.rs`

```rust
"task" => (RiskLevel::AutoAllow, String::new()),
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`

```ts
import { ListChecks } from 'lucide-react';
task: { Icon: ListChecks, colorClass: 'text-emerald-400' },
```

### `src/screens/AgentInspector/ConfigTab.tsx`

Add new "Task Management" category:

```ts
{
    label: 'Task Management',
    tools: [{ id: 'task', label: 'Agent Task Tracking' }],
},
```

### `src/components/chat/ToolUseBlock.tsx`

Render task results through the shared tool presentation system from plan [26-tool-use-ui](26-tool-use-ui.md). This plan should add the task-specific formatter needed for compact task chips / a mini task board.

## Permission Level

**AutoAllow** — internal tracking, no external side effects.

## Dependencies

- New DB table + migration
- `uuid` (already available)

## Key Design Decisions

- **Single tool** with actions (vs. Claude Code's 7 tools) — reduces tool definition bloat, simpler for the LLM
- **Scoped to session** — tasks are per-session, not global. Agents start fresh each session.
- **Separate from scheduler tasks** — do not reuse the existing scheduled-task tables/types even though the tool name is `task`
- **Dependency tracking** via `blocked_by` — agents can express task ordering
- **UI integration** — task status shown in the chat as compact chips

## Verification

1. `task { action: "create", subject: "Write tests", active_form: "Writing tests" }` -> returns task_id
2. `task { action: "list" }` -> shows all tasks with statuses
3. `task { action: "update", task_id: "...", status: "in_progress" }` -> updates status
4. `task { action: "update", task_id: "...", status: "completed" }` -> marks done
5. `task { action: "delete", task_id: "..." }` -> removes task
6. Test blocked_by dependency chain
