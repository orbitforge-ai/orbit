# Plan: `ask_user` Tool — Structured User Questions

> **Source**: Claude Code's `AskUserQuestion` tool. Not present in OpenClaw.
> **Approach**: New tool — agents currently have no way to ask the user structured questions.
> **Phase note**: This is not a Phase 1 quick win. It depends on the same pause/resume runtime machinery as `yield_turn`, plus dedicated frontend state for pending questions.

## Context

Claude Code's AskUserQuestion lets agents present structured choices to the user (multiple choice, custom text, multi-select). Orbit agents can only communicate through their text output and the `finish` tool. When an agent needs clarification, it must either guess or use `finish` and hope the user restarts with more context. A dedicated tool lets agents pause, ask, and continue.

## What It Does

Present a question to the user with optional choices. The agent pauses until the user responds, then continues with the answer. Supports: free text questions, multiple choice, multi-select, and questions with previews/context.

## Backend Changes

### New file: `src-tauri/src/executor/tools/ask_user.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod ask_user;` and `Box::new(ask_user::AskUserTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):

```rust
ToolDefinition {
    name: "ask_user".to_string(),
    description: "Ask the user a question and wait for their response. Use for clarification, choosing between approaches, or getting required input. The agent pauses until the user answers.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "question": {
                "type": "string",
                "description": "The question to ask the user"
            },
            "choices": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Optional list of choices. If omitted, user provides free text."
            },
            "allow_custom": {
                "type": "boolean",
                "description": "If true and choices are provided, also allow a custom text response. Default: true."
            },
            "multi_select": {
                "type": "boolean",
                "description": "If true, user can select multiple choices. Default: false."
            },
            "context": {
                "type": "string",
                "description": "Optional additional context shown below the question (e.g., code preview, diff)"
            }
        },
        "required": ["question"]
    }),
}
```

**Execution** (in `execute()` method):

```rust
"ask_user" => {
    let question = input["question"].as_str().ok_or("ask_user: missing question")?;
    let choices = input["choices"].as_array().map(|arr|
        arr.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>()
    );
    let allow_custom = input["allow_custom"].as_bool().unwrap_or(true);
    let multi_select = input["multi_select"].as_bool().unwrap_or(false);
    let context = input["context"].as_str();

    // Emit an event to the frontend with the question
    let request_id = uuid::Uuid::new_v4().to_string();
    if let Some(app) = &ctx.app {
        emit_user_question(app, &request_id, ctx.current_run_id.as_deref().unwrap_or(""),
            question, choices.as_deref(), allow_custom, multi_select, context).await;
    }

    // Wait for the user's response using a dedicated question registry.
    // This should share generic pause/resume plumbing with yield_turn,
    // but should not overload PermissionRegistry directly.
    let registry = ctx.user_question_registry.as_ref()
        .ok_or("ask_user: no question registry")?;
    let response = wait_for_user_response(&request_id, registry).await
        .map_err(|_| "ask_user: timed out or cancelled".to_string())?;

    Ok((response, false))
}
```

### Events: `src-tauri/src/events/emitter.rs`

New event:

```rust
pub async fn emit_user_question(
    app: &tauri::AppHandle,
    request_id: &str,
    run_id: &str,
    question: &str,
    choices: Option<&[String]>,
    allow_custom: bool,
    multi_select: bool,
    context: Option<&str>,
) { ... }
```

### New Tauri command for response:

```rust
#[tauri::command]
pub async fn respond_to_user_question(
    request_id: String,
    response: String,
    registry: State<'_, UserQuestionRegistry>,
) -> Result<(), String> { ... }
```

### Runtime dependency

This plan should share a generic "paused run waiting on external input" mechanism with `yield_turn` rather than introducing a second bespoke suspension path. The minimal architecture is:

- `yield_turn` and `ask_user` both produce a wait condition
- the session/agent loop parks the run and records that condition
- the relevant event source resumes the run with structured input

### `src-tauri/src/executor/permissions.rs`

```rust
"ask_user" => (RiskLevel::AutoAllow, String::new()),
```

## Frontend Changes

### New component: `src/components/chat/UserQuestionPrompt.tsx`

Similar to `PermissionPrompt.tsx` but for user questions:
- Display the question text
- If choices: render as clickable buttons/chips
- If multi_select: checkboxes
- If allow_custom: text input field
- Optional context block (scrollable, monospace)
- Submit button sends response back via Tauri command

### `src/components/chat/toolVisuals.ts`

```ts
import { HelpCircle } from 'lucide-react';
ask_user: { Icon: HelpCircle, colorClass: 'text-blue-400' },
```

### `src/components/chat/ToolUseBlock.tsx`

The inline `UserQuestionPrompt` is required for the tool to function and remains in scope here. Build it within the shared tool presentation foundation from plan [26-tool-use-ui](26-tool-use-ui.md), and add the compact resolved-state presentation through that same shared system.

### `src/screens/AgentInspector/ConfigTab.tsx`

Add to "Communication" category:

```ts
{ id: 'ask_user', label: 'Ask User' },
```

## Permission Level

**AutoAllow** — no side effects, just pauses for user input.

## Dependencies

- New event type + Tauri command for the question/response flow
- Dedicated `UserQuestionRegistry` or a generic wait registry shared with `yield_turn`
- Session/agent loop pause-resume support

## Key Design Decisions

- **Reuses the pause/resume pattern, not the permission registry itself**: the flow is similar to permission prompts, but it should have its own registry/state model
- **Inline in chat**: Questions appear as interactive elements within the chat flow, not as modal dialogs
- **Timeout**: Optional timeout (default: no timeout — waits indefinitely until user responds or session cancelled)
- **Sub-agents**: Sub-agents should NOT use ask_user (they can't interact with the user directly). Block via `is_sub_agent` check.

## Verification

1. `ask_user { question: "Which approach should I take?" }` -> free text prompt appears in chat
2. `ask_user { question: "Pick a framework:", choices: ["React", "Vue", "Svelte"] }` -> buttons appear
3. User clicks choice -> agent receives response and continues
4. `ask_user` in sub-agent context -> blocked with error
5. Session cancelled while waiting -> agent loop terminates cleanly
