# Coding Session Checklists for Missing-Tools Work

This file is a handoff guide for running focused coding sessions against the missing-tools roadmap without overloading context.

Use these checklists as copy-paste prompts for implementation sessions. Each session prompt explicitly names the plan files to read first and the files/code areas that should stay in scope.

## How To Use This File

- Run one checklist per coding session
- Keep the session tightly scoped to the files and acceptance criteria listed
- Do not preload every plan file; only provide the plan files called out in that checklist
- Prefer finishing one layer of work at a time:
  - shared UI foundation
  - existing built-in tool formatters
  - one missing-tool plan at a time, including its formatter hookup

## Session 1 — Shared Tool-Use UI Foundation

### Read These Plan Files First

- [26-tool-use-ui.md](26-tool-use-ui.md)
- [README.md](README.md)

### Primary Code Areas

- `/Users/matwaroff/Code/orbit/src/components/chat/ToolUseBlock.tsx`
- `/Users/matwaroff/Code/orbit/src/store/settingsStore.ts`
- `/Users/matwaroff/Code/orbit/src/screens/Settings/index.tsx`
- one new helper file under `/Users/matwaroff/Code/orbit/src/components/chat/` such as `toolPresentation.ts`

### Goal

Build the shared tool presentation foundation from plan 26 so new tools can plug into a consistent UI instead of inventing one-off `ToolUseBlock` rendering.

### Checklist

- Add a new global setting for verbose/debug tool details
- Persist that setting in local storage
- Add a Settings toggle with clear copy explaining normal mode vs verbose/debug mode
- Move raw input/result display logic out of `ToolUseBlock.tsx` into a helper layer
- Add result normalization that strips persisted `<tool_result ...>...</tool_result>` wrappers
- Add a formatter registry shape for tool-specific presenters
- Add a generic fallback presenter for unknown tools
- Add chip expandability rules so status/action tools can stay chip-only by default
- Preserve current pending, error, and interrupted-state behavior

### Acceptance Criteria

- Tool details no longer default to raw JSON + raw string only
- Raw payloads are hidden unless verbose/debug mode is enabled
- Unknown tools still render in a readable fallback format
- Existing chips still expand/collapse correctly
- `npm run build` passes

### Copy-Paste Prompt

```text
Implement the shared tool-use UI foundation described in:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md

Focus only on the shared presentation framework, not bespoke polish for every tool.

Files in scope:
- src/components/chat/ToolUseBlock.tsx
- src/store/settingsStore.ts
- src/screens/Settings/index.tsx
- one new helper file under src/components/chat/ for tool presentation/normalization

Requirements:
- add a global verbose/debug toggle for tool details, default false
- normalize persisted tool_result wrappers before rendering
- replace raw JSON/raw string as the default detail presentation
- add a formatter registry shape and a generic fallback renderer
- keep raw payloads visible only in debug mode
- preserve pending/error/interrupted behavior and existing chip expand/collapse behavior

Out of scope:
- no tool-specific polish beyond the generic fallback system
- no backend changes
- no unrelated chat UI redesign

Acceptance criteria:
- human-readable default tool detail view exists
- raw payloads show only in debug mode
- unknown tools still render readably
- npm run build passes
```

## Session 2 — Existing Built-In Tool Formatters

### Read These Plan Files First

- [26-tool-use-ui.md](26-tool-use-ui.md)
- [README.md](README.md)

### Primary Code Areas

- `/Users/matwaroff/Code/orbit/src/components/chat/ToolUseBlock.tsx`
- the new helper file from Session 1
- optional small presentational helpers/components under `/Users/matwaroff/Code/orbit/src/components/chat/`

### Goal

Use the plan 26 foundation to improve the noisiest already-shipped tools first so the chat UI gets immediate value before missing-tool backend work starts.

### Checklist

- Add tailored input/output rendering for `search_memory`
- Add tailored input/output rendering for `list_memories`
- Add tailored input/output rendering for `shell_command`
- Add tailored input/output rendering for `read_file`
- Add tailored input/output rendering for `write_file`
- Add tailored input/output rendering for `list_files`
- Add tailored input/output rendering for `web_search`
- Keep `remember`, `forget`, `finish`, and `activate_skill` chip-only in normal mode
- Preserve raw payload access in debug mode

### Acceptance Criteria

- memory tools render entries as readable rows/cards
- `shell_command` renders command, stdout/stderr, and exit code in sections
- `read_file` renders file content readably in normal mode
- `write_file` hides full file content in normal mode and shows a concise summary
- `list_files` renders a readable listing/table view
- `web_search` renders result cards with title, URL, and snippet
- `npm run build` passes

### Copy-Paste Prompt

```text
Build on the shared tool-use UI foundation from:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md

Implement dedicated formatters for existing built-in tools only:
- search_memory
- list_memories
- shell_command
- read_file
- write_file
- list_files
- web_search

Keep these chip-only in normal mode:
- remember
- forget
- finish
- activate_skill

Files in scope:
- src/components/chat/ToolUseBlock.tsx
- the shared tool presentation helper(s) created for plan 26
- optional small presentational helpers under src/components/chat/

Requirements:
- each listed tool gets a tailored human-readable input/output view
- raw input/result remains available only in debug mode
- do not touch backend code
- do not redesign unrelated chat surfaces

Acceptance criteria:
- no listed tool defaults to raw payloads in normal mode
- pending/error/interrupted states still work
- npm run build passes
```

## Session 3 Template — One Missing Tool Plan + Formatter

Run this once per tool plan after Sessions 1 and 2 are done.

### Read These Plan Files First

- [26-tool-use-ui.md](26-tool-use-ui.md)
- [README.md](README.md)
- the specific missing-tool plan for the tool you are implementing

Examples:
- MCP: [23-mcp.md](23-mcp.md)
- Task: [20-task-management.md](20-task-management.md)
- Schedule: [25-scheduling.md](25-scheduling.md)
- Browser: [15-browser.md](15-browser.md)
- Yield: [14-yield.md](14-yield.md)
- Ask user: [22-ask-user.md](22-ask-user.md)
- Notebook support: [24-notebook-edit.md](24-notebook-edit.md)
- Image generation: [06-image-generation.md](06-image-generation.md)

### Goal

Implement one tool/expansion end to end and wire it into the shared plan 26 presentation system instead of creating bespoke `ToolUseBlock` UI.

### Checklist

- Implement the backend/tool behavior from the tool’s plan file
- Implement the concrete runtime/data/config changes described in that plan file, not just the tool formatter
- Return stable, presentation-friendly result payloads where reasonable
- Add/update the tool icon in `toolVisuals.ts`
- Add the tool enablement/category wiring described in the tool plan
- Add a dedicated formatter/renderer for that tool inside the shared plan 26 presentation system
- Keep raw payload visibility handled by the shared debug mode rather than local ad hoc UI

### Acceptance Criteria

- the tool works end to end
- the tool renders through the shared tool presentation system
- the tool’s expanded view is human-readable in normal mode
- raw payloads remain available in debug mode
- `npm run build` passes

### Copy-Paste Prompt Template

```text
Implement the missing-tool plan and shared UI integration for <tool_name>.

Read these plan files first and follow them:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md
- plans/missing-tools/<plan-file>.md

Use plan 26 as the required presentation architecture.
Do not invent one-off ToolUseBlock UI for this tool.

Requirements:
- implement the backend/tool behavior from the tool plan
- implement the concrete runtime/data/config changes described in that plan file, not just the UI integration
- add/update any tool registration, permissions, visuals, and config wiring required by the tool plan
- add a dedicated formatter/renderer for this tool inside the shared tool presentation system from plan 26
- keep raw payload visibility controlled by the shared debug mode

Out of scope:
- do not redesign unrelated tools
- do not bypass the shared tool presentation architecture

Acceptance criteria:
- tool works end to end
- expanded tool UI is human-readable in normal mode
- raw payloads remain available in debug mode
- npm run build passes
```

## Suggested Run Order

### First

1. Session 1 — shared tool-use UI foundation
2. Session 2 — built-in formatter pass

### Then prefer these bundled sessions

1. plan 00 — refactor foundation
2. plan 03 — shell/background execution expansion
3. plans 01 + 17 + 18 — file editing/search bundle
4. plans 02 + 05 — web fetch + image analysis bundle
5. plans 08 + 10 + 11 — session inspection bundle
6. plans 09 + 12 + 13 — session control bundle
7. plans 14 + 22 — pause/resume + ask-user bundle
8. plans 16 + 21 — self-management + worktree bundle
9. plan 20 — task
10. plan 25 — schedule
11. plan 23 — MCP
12. plan 07 — external message channels
13. plan 24 — notebook support
14. plan 06 — image generation
15. plan 15 — browser
16. plan 19 — LSP

This reduces the active implementation sequence to 16 focused sessions including the two UI foundation sessions, instead of running a separate session for every plan file.

## Plan Coverage Map

Use this map to confirm every plan is accounted for.

| Plan | Recommended session |
|---|---|
| 00 | standalone |
| 01 | file editing/search bundle |
| 02 | web fetch + image analysis bundle |
| 03 | standalone |
| 04 | merged into 03 |
| 05 | web fetch + image analysis bundle |
| 06 | standalone |
| 07 | standalone |
| 08 | session inspection bundle |
| 09 | session control bundle |
| 10 | session inspection bundle |
| 11 | session inspection bundle |
| 12 | session control bundle |
| 13 | session control bundle |
| 14 | pause/resume + ask-user bundle |
| 15 | standalone |
| 16 | self-management + worktree bundle |
| 17 | file editing/search bundle |
| 18 | file editing/search bundle |
| 19 | standalone |
| 20 | standalone |
| 21 | self-management + worktree bundle |
| 22 | pause/resume + ask-user bundle |
| 23 | standalone |
| 24 | standalone |
| 25 | standalone |
| 26 | Session 1 |

## Ready-to-Use Session Prompts

### Bundled Sessions First

### File Editing/Search Bundle (Plans 01 + 17 + 18)

Plan scope to implement:
- add the `edit_file` tool with exact replacement semantics
- expand `list_files` with glob pattern support
- add the `grep` tool for content search
- add permissions/visuals/config wiring for the new tools
- add formatter support in the shared tool presentation system

```text
Implement the file editing/search bundle.

Read these plan files first and follow them:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md
- plans/missing-tools/01-edit-file.md
- plans/missing-tools/17-glob.md
- plans/missing-tools/18-grep.md

Use plan 26 as the required presentation architecture.

Requirements:
- implement edit_file with exact replace / replace_all behavior
- expand list_files with glob pattern support
- implement grep for content search with pattern/glob/output modes
- add/update permissions, visuals, and config wiring required by the plans
- add dedicated formatter/rendering support for edit_file, globbed list_files output, and grep

Acceptance criteria:
- edit_file, globbed list_files, and grep work end to end
- outputs are human-readable in normal mode
- raw payloads remain available in debug mode
- npm run build passes
```

### Web Fetch + Image Analysis Bundle (Plans 02 + 05)

Plan scope to implement:
- add `web_fetch` with SSRF-safe URL fetching and HTML-to-markdown extraction
- extract/reuse shared SSRF-safe fetch helpers instead of duplicating logic
- add `image_analysis` with workspace-path and remote-image support
- route image analysis through a provider-aware one-shot vision call
- add permissions/visuals/config wiring and shared formatter support

```text
Implement the web fetch + image analysis bundle.

Read these plan files first and follow them:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md
- plans/missing-tools/02-web-fetch.md
- plans/missing-tools/05-image-analysis.md

Use plan 26 as the required presentation architecture.

Requirements:
- implement web_fetch with SSRF-safe fetch logic and HTML-to-markdown extraction
- extract shared network/SSRF helpers if needed instead of duplicating them
- implement image_analysis for workspace images and remote URLs
- route image_analysis through a provider-aware one-shot vision call
- add/update permissions, visuals, and config wiring required by the plans
- add formatter/rendering support for web_fetch and image_analysis

Acceptance criteria:
- both tools work end to end
- remote fetch/image paths are validated safely
- outputs are human-readable in normal mode
- raw payloads remain available in debug mode
- npm run build passes
```

### Session Inspection Bundle (Plans 08 + 10 + 11)

Plan scope to implement:
- add `session_history`
- add `session_status`
- add `sessions_list`
- share DB queries/helpers for session lookup, listing, message history, and run stats where possible
- keep access agent-scoped and read-only
- add formatter support in the shared tool presentation system

```text
Implement the session inspection bundle.

Read these plan files first and follow them:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md
- plans/missing-tools/08-session-history.md
- plans/missing-tools/10-session-status.md
- plans/missing-tools/11-sessions-list.md

Use plan 26 as the required presentation architecture.

Requirements:
- implement session_history, session_status, and sessions_list
- share DB query/helpers for session lookup/listing/history/stats where reasonable
- enforce agent-scoped read-only access rules from the plans
- add/update permissions, visuals, and config wiring required by the plans
- add formatter/rendering support for session history, status cards, and session lists

Acceptance criteria:
- all three tools work end to end
- outputs are human-readable in normal mode
- access control is enforced
- raw payloads remain available in debug mode
- npm run build passes
```

### Session Control Bundle (Plans 09 + 12 + 13)

Plan scope to implement:
- add `session_send`
- add `sessions_spawn`
- add `subagents`
- reuse existing session creation/execution infrastructure where possible
- enforce session relationship/security rules
- add formatter support in the shared tool presentation system

```text
Implement the session control bundle.

Read these plan files first and follow them:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md
- plans/missing-tools/09-session-send.md
- plans/missing-tools/12-sessions-spawn.md
- plans/missing-tools/13-subagents-management.md

Use plan 26 as the required presentation architecture.

Requirements:
- implement session_send for appending to existing sessions
- implement sessions_spawn for one-shot and persistent session spawning
- implement subagents management for list/kill/steer
- reuse existing session creation/execution infrastructure where possible
- enforce access/relationship rules from the plans
- add/update permissions, visuals, and config wiring required by the plans
- add formatter/rendering support for these tools in the shared tool presentation system

Acceptance criteria:
- all three tools work end to end
- persistent sessions can be spawned and interacted with
- sub-agent management works
- outputs are human-readable in normal mode
- raw payloads remain available in debug mode
- npm run build passes
```

### Pause/Resume + Ask-User Bundle (Plans 14 + 22)

Plan scope to implement:
- add `yield_turn`
- add `ask_user`
- build the shared pause/resume runtime plumbing once
- add wait registries/events/response flow
- render waiting/question states through the shared tool presentation system

```text
Implement the pause/resume + ask-user bundle.

Read these plan files first and follow them:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md
- plans/missing-tools/14-yield.md
- plans/missing-tools/22-ask-user.md

Use plan 26 as the required presentation architecture.

Requirements:
- implement yield_turn and ask_user together
- build the shared pause/resume runtime plumbing once and reuse it for both tools
- implement user-question event/response flow and required waiting registries
- add/update permissions, visuals, and config wiring required by the plans
- keep required waiting/question UI functional in normal mode through the shared presentation system

Acceptance criteria:
- both tools work end to end
- pause/resume behavior is reliable
- interactive question flow works
- outputs/states are human-readable in normal mode
- raw payloads remain available in debug mode
- npm run build passes
```

### Self-Management + Worktree Bundle (Plans 16 + 21)

Plan scope to implement:
- add the `config` tool replacing old gateway semantics
- add the `worktree` tool
- support agent self-inspection/config mutation with guarded setting whitelist
- support git worktree lifecycle and workspace switching
- add formatter support in the shared tool presentation system

```text
Implement the self-management + worktree bundle.

Read these plan files first and follow them:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md
- plans/missing-tools/16-gateway.md
- plans/missing-tools/21-worktree.md

Use plan 26 as the required presentation architecture.

Requirements:
- implement the config tool with get/set/list/info actions and the allowed/blocked setting model from the plan
- implement the worktree tool with create/exit/list actions
- support workspace switching/worktree lifecycle as described in the worktree plan
- add/update permissions, visuals, and config wiring required by the plans
- add formatter/rendering support for config and worktree inside the shared tool presentation system

Acceptance criteria:
- both tools work end to end
- guarded config changes are enforced
- worktree lifecycle works in git repositories
- outputs are human-readable in normal mode
- raw payloads remain available in debug mode
- npm run build passes
```

### Standalone Sessions

### Refactor Foundation (Plan 00)

Plan scope to implement:
- refactor `src-tauri/src/executor/agent_tools.rs` into per-tool modules under `src-tauri/src/executor/tools/`
- move `ToolExecutionContext` to `tools/context.rs`
- move shared helpers into `tools/helpers.rs`
- introduce the `ToolHandler` trait and slim orchestration layer
- keep the public API stable so existing callers continue to work

```text
Implement the agent-tools refactor foundation.

Read these plan files first and follow them:
- plans/missing-tools/00-refactor-agent-tools.md
- plans/missing-tools/README.md

Requirements:
- refactor the monolithic agent_tools.rs into per-tool modules under src-tauri/src/executor/tools/
- introduce the ToolHandler trait and slim orchestrator structure from the plan
- move ToolExecutionContext and shared helpers into the planned module layout
- preserve the public API surface expected by existing callers
- keep current behavior unchanged while restructuring

Out of scope:
- do not add new missing tools yet
- do not redesign existing tool behavior

Acceptance criteria:
- build_tool_definitions and execute_tool still work through the new structure
- existing tools compile and behave the same
- npm run build and relevant Rust checks pass
```

### Shell Execution Expansion (Plan 03)

Plan scope to implement:
- expand `shell_command` with `run_in_background`, `process_action`, and `process_id`
- add `BgProcessRegistry` / background-process tracking
- support process `list`, `poll`, and `kill`
- update permissions for process actions
- render the new shell/process states through the shared tool presentation system

```text
Implement the shell execution expansion and shared UI integration.

Read these plan files first and follow them:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md
- plans/missing-tools/03-exec-background.md

Use plan 26 as the required presentation architecture.

Requirements:
- expand shell_command with run_in_background/process_action/process_id support
- add the background-process registry/runtime support from the plan
- implement list/poll/kill behavior for managed processes
- update permissions handling for process actions
- add the shell/background-process formatter inside the shared tool presentation system

Acceptance criteria:
- blocking shell_command still works
- background execution and process management work end to end
- expanded tool UI is human-readable in normal mode
- raw payloads remain available in debug mode
- npm run build passes
```

### MCP

Plan scope to implement:
- add the unified `mcp` tool with actions `list_servers`, `list_tools`, `call_tool`, `list_resources`, `read_resource`
- add MCP client/manager support for configured servers and server lifecycle
- add permissions classification for discovery vs `call_tool`
- add MCP server configuration UI in settings and tool enablement wiring
- add an MCP formatter in the shared tool presentation system for list/read/call actions

```text
Implement the missing-tool plan and shared UI integration for mcp.

Read these plan files first and follow them:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md
- plans/missing-tools/23-mcp.md

Use plan 26 as the required presentation architecture.
Do not invent one-off ToolUseBlock UI for this tool.

Requirements:
- implement the backend/tool behavior from the MCP plan
- implement the actual MCP plan scope:
  - unified `mcp` tool with list/call/read actions
  - MCP client/manager support for configured servers
  - permissions classification for discovery vs call_tool
  - settings/config UI for MCP servers
- add/update tool registration, permissions, visuals, and config wiring required by the plan
- add a dedicated formatter/renderer for mcp inside the shared tool presentation system
- make sure list/read/call actions render human-readably in normal mode
- keep raw payload visibility controlled by the shared debug mode

Acceptance criteria:
- mcp works end to end
- expanded tool UI is human-readable in normal mode
- raw payloads remain available in debug mode
- npm run build passes
```

### Task

Plan scope to implement:
- add the `task` tool with actions `create`, `list`, `get`, `update`, `delete`
- add the `agent_tasks` database table/migration and supporting query helpers
- scope tasks to the current session/agent and keep them separate from scheduler tasks
- add permissions, visuals, and tool enablement wiring
- add a task formatter in the shared tool presentation system

```text
Implement the missing-tool plan and shared UI integration for task.

Read these plan files first and follow them:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md
- plans/missing-tools/20-task-management.md

Use plan 26 as the required presentation architecture.
Do not invent one-off ToolUseBlock UI for this tool.

Requirements:
- implement the backend/tool behavior from the task plan
- implement the actual task plan scope:
  - `task` tool with create/list/get/update/delete actions
  - `agent_tasks` table/migration and supporting DB access
  - session-scoped task tracking separate from scheduler tasks
- add/update tool registration, permissions, visuals, and config wiring required by the plan
- add a dedicated formatter/renderer for task inside the shared tool presentation system
- render task create/list/get/update/delete results as readable task rows/cards in normal mode
- keep raw payload visibility controlled by the shared debug mode

Acceptance criteria:
- task works end to end
- expanded tool UI is human-readable in normal mode
- raw payloads remain available in debug mode
- npm run build passes
```

### Schedule

Plan scope to implement:
- add the unified `schedule` tool with task-schedule actions and pulse actions
- reuse existing scheduler and pulse command-layer logic instead of duplicating it
- enforce ownership so agents only manage their own tasks/schedules/pulse
- add permissions, visuals, and tool enablement wiring
- add a schedule formatter in the shared tool presentation system

```text
Implement the missing-tool plan and shared UI integration for schedule.

Read these plan files first and follow them:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md
- plans/missing-tools/25-scheduling.md

Use plan 26 as the required presentation architecture.
Do not invent one-off ToolUseBlock UI for this tool.

Requirements:
- implement the backend/tool behavior from the schedule plan
- implement the actual schedule plan scope:
  - unified `schedule` tool for generic schedules and pulse management
  - reuse existing scheduler/pulse backend flows
  - enforce agent ownership checks for schedule mutations
- add/update tool registration, permissions, visuals, and config wiring required by the plan
- add a dedicated formatter/renderer for schedule inside the shared tool presentation system
- render list/preview/pulse results as readable schedule tables/cards in normal mode
- keep raw payload visibility controlled by the shared debug mode

Acceptance criteria:
- schedule works end to end
- expanded tool UI is human-readable in normal mode
- raw payloads remain available in debug mode
- npm run build passes
```

### Browser

Plan scope to implement:
- add browser manager/runtime support for headless browser sessions and page tracking
- add the `browser` tool with actions like `navigate`, `click`, `type`, `screenshot`, `get_content`, `evaluate`, `scroll`, `wait_for`, `close`
- add browser-specific permission classification
- add visuals and tool enablement wiring
- add a browser formatter in the shared tool presentation system

```text
Implement the missing-tool plan and shared UI integration for browser.

Read these plan files first and follow them:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md
- plans/missing-tools/15-browser.md

Use plan 26 as the required presentation architecture.
Do not invent one-off ToolUseBlock UI for this tool.

Requirements:
- implement the backend/tool behavior from the browser plan
- implement the actual browser plan scope:
  - browser manager/runtime support
  - browser tool actions for navigation, interaction, screenshots, content, JS eval, waits
  - browser-specific permissions
- add/update tool registration, permissions, visuals, and config wiring required by the plan
- add a dedicated formatter/renderer for browser inside the shared tool presentation system
- render screenshot previews, page-state metadata, and action-specific results readably in normal mode
- keep raw payload visibility controlled by the shared debug mode

Acceptance criteria:
- browser works end to end
- expanded tool UI is human-readable in normal mode
- raw payloads remain available in debug mode
- npm run build passes
```

### Yield Turn

Plan scope to implement:
- add the `yield_turn` tool and schema
- modify the session/agent loop to support pausing and resuming on sub-agents, timeout, or message triggers
- add permissions, visuals, and tool enablement wiring
- keep a visible waiting-state UI and add a yield formatter in the shared tool presentation system

```text
Implement the missing-tool plan and shared UI integration for yield_turn.

Read these plan files first and follow them:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md
- plans/missing-tools/14-yield.md

Use plan 26 as the required presentation architecture.
Do not invent one-off ToolUseBlock UI for this tool.

Requirements:
- implement the backend/tool behavior from the yield_turn plan
- implement the actual yield_turn plan scope:
  - `yield_turn` tool and schema
  - agent/session loop pause-resume support for wait conditions
  - required waiting-state runtime behavior
- add/update tool registration, permissions, visuals, and config wiring required by the plan
- add a dedicated formatter/renderer for yield_turn inside the shared tool presentation system
- preserve the required waiting state and timer/status UI in normal mode
- keep raw payload visibility controlled by the shared debug mode

Acceptance criteria:
- yield_turn works end to end
- paused/waiting state is understandable in normal mode
- raw payloads remain available in debug mode
- npm run build passes
```

### Ask User

Plan scope to implement:
- add the `ask_user` tool and schema
- add user-question events, response command, and question registry / wait plumbing
- reuse the generic pause-resume pattern shared with `yield_turn`
- add the inline `UserQuestionPrompt` UI, permissions, visuals, and tool enablement wiring
- add ask-user presentation inside the shared tool presentation system

```text
Implement the missing-tool plan and shared UI integration for ask_user.

Read these plan files first and follow them:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md
- plans/missing-tools/22-ask-user.md

Use plan 26 as the required presentation architecture.
Do not invent one-off ToolUseBlock UI for this tool.

Requirements:
- implement the backend/tool behavior from the ask_user plan
- implement the actual ask_user plan scope:
  - `ask_user` tool and schema
  - user-question event/response plumbing and registry
  - pause-resume integration for waiting on answers
  - inline interactive UserQuestionPrompt UI
- add/update tool registration, permissions, visuals, and config wiring required by the plan
- add a dedicated formatter/renderer for ask_user inside the shared tool presentation system
- keep the inline interactive UserQuestionPrompt functional in normal mode
- keep raw payload visibility controlled by the shared debug mode

Acceptance criteria:
- ask_user works end to end
- inline question flow is usable and readable in normal mode
- raw payloads remain available in debug mode
- npm run build passes
```

### Notebook Support

Plan scope to implement:
- expand `read_file` to understand `.ipynb` notebooks
- expand `edit_file` with notebook-aware cell operations from the notebook plan
- keep existing non-notebook file behavior unchanged
- add notebook-aware rendering inside the shared tool presentation system

```text
Implement the notebook support plan and shared UI integration for notebook-aware read_file/edit_file.

Read these plan files first and follow them:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md
- plans/missing-tools/24-notebook-edit.md
- plans/missing-tools/01-edit-file.md

Use plan 26 as the required presentation architecture.
Do not invent one-off ToolUseBlock UI for notebook output.

Requirements:
- implement the actual notebook support scope from the plan:
  - notebook-aware `read_file`
  - notebook-aware `edit_file` cell operations
  - preserve normal non-notebook file behavior
- update the shared tool presentation system with a notebook-aware renderer for read_file/edit_file output
- render notebook cells and outputs readably in normal mode
- keep raw payload visibility controlled by the shared debug mode

Acceptance criteria:
- notebook-aware read/edit flows work end to end
- notebook cell output is readable in normal mode
- raw payloads remain available in debug mode
- npm run build passes
```

### Image Generation

Plan scope to implement:
- add the `image_generation` tool and provider helper
- add dedicated image-generation provider/settings and API-key storage
- keep image generation separate from chat provider/model configuration
- add permissions, visuals, and tool enablement wiring
- add an image-generation formatter in the shared tool presentation system

```text
Implement the missing-tool plan and shared UI integration for image_generation.

Read these plan files first and follow them:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md
- plans/missing-tools/06-image-generation.md

Use plan 26 as the required presentation architecture.
Do not invent one-off ToolUseBlock UI for this tool.

Requirements:
- implement the backend/tool behavior from the image_generation plan
- implement the actual image_generation plan scope:
  - `image_generation` tool and provider helper
  - dedicated image-generation settings/provider/key storage
  - separate image-generation config from chat model config
- add/update tool registration, permissions, visuals, and settings/config wiring required by the plan
- add a dedicated formatter/renderer for image_generation inside the shared tool presentation system
- render saved-image previews and output-path metadata readably in normal mode
- keep raw payload visibility controlled by the shared debug mode

Acceptance criteria:
- image_generation works end to end
- expanded tool UI is human-readable in normal mode
- raw payloads remain available in debug mode
- npm run build passes
```

### External Message Channels (Plan 07)

Plan scope to implement:
- add the `message` tool with `list` and `send`
- add external channel config/runtime support for Slack/Discord/webhook, with email optional if practical
- add channel configuration UI and permissions/visuals wiring
- add formatter support in the shared tool presentation system

```text
Implement the external message channels plan and shared UI integration.

Read these plan files first and follow them:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md
- plans/missing-tools/07-message-channels.md

Use plan 26 as the required presentation architecture.

Requirements:
- implement the message tool with list/send actions
- implement channel config/runtime support for Slack, Discord, and generic webhooks
- include email only if it fits cleanly; otherwise keep the implementation aligned with the plan's phase-1/phase-2 guidance
- add channel configuration UI plus permissions, visuals, and config wiring
- add formatter/rendering support for message tool output in the shared presentation system

Acceptance criteria:
- message works end to end for configured external channels
- dangerous sends are permission-gated
- outputs are human-readable in normal mode
- raw payloads remain available in debug mode
- npm run build passes
```

### LSP (Plan 19)

Plan scope to implement:
- add LSP manager/runtime support
- add the `lsp` tool with navigation/intelligence actions
- auto-detect language/server and manage server lifecycle
- add permissions/visuals/config wiring
- add formatter support in the shared tool presentation system

```text
Implement the LSP plan and shared UI integration.

Read these plan files first and follow them:
- plans/missing-tools/26-tool-use-ui.md
- plans/missing-tools/README.md
- plans/missing-tools/19-lsp.md

Use plan 26 as the required presentation architecture.

Requirements:
- implement the LSP manager/runtime support from the plan
- implement the lsp tool actions for definition/reference/hover/symbol operations
- handle language detection, server startup, and server lifecycle as described in the plan
- add/update permissions, visuals, and config wiring required by the plan
- add formatter/rendering support for LSP results in the shared tool presentation system

Acceptance criteria:
- lsp works end to end when a language server is available
- missing-server errors are clear
- outputs are human-readable in normal mode
- raw payloads remain available in debug mode
- npm run build passes
```
