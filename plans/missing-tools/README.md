# Missing Tools — Implementation Plans

Gap analysis comparing Orbit's agent tools against **OpenClaw** and **Claude Code** tool sets. Plans are organized as either **expansions of existing Orbit tools** or **new tools**.

## Sources Analyzed

| System | Tool Count | Key Strengths |
|--------|-----------|---------------|
| **Orbit** (current) | 14 tools | Agent bus, sub-agents, memory, skills, permissions |
| **OpenClaw** | 20 tools | Browser, sessions, process mgmt, image gen |
| **Claude Code** | 42 tools | LSP, grep, glob, MCP, tasks, worktrees, teams |

## Reconciliation Strategy

Where tools overlap across systems, we **expand existing Orbit tools** rather than creating new ones:

| Existing Tool | Expansion | Source |
|---------------|-----------|--------|
| `shell_command` | + `run_in_background`, `process_action` | Claude Code Bash + OpenClaw Exec/Process |
| `list_files` | + `pattern` (glob matching) | Claude Code Glob |
| `read_file` | + `.ipynb` notebook support | Claude Code Read + NotebookEdit |
| `edit_file` (new) | + `notebook_action` for .ipynb | Claude Code Edit + NotebookEdit |
| `config` (was gateway) | Reconciled with Claude Code Config | OpenClaw Gateway + Claude Code Config |

## Priority Order

### Phase 1 — Quick Wins (bounded scope, high value)

| # | Plan | Type | Description |
|---|------|------|-------------|
| 01 | [edit_file](01-edit-file.md) | New tool | Targeted find-and-replace file editing |
| 02 | [web_fetch](02-web-fetch.md) | New tool | Fetch URL content as markdown/text |
| 17 | [glob (expand list_files)](17-glob.md) | Expand | Add glob pattern matching to list_files |
| 18 | [grep](18-grep.md) | New tool | Content search with regex |

### Phase 2 — Medium Complexity (builds on existing patterns and runtime pause/resume)

| # | Plan | Type | Description |
|---|------|------|-------------|
| 03 | [shell_command expansion](03-exec-background.md) | Expand | Background exec + process management (merges old 03+04) |
| ~~04~~ | ~~process~~ | ~~Merged~~ | ~~Merged into plan 03~~ |
| 08 | [session_history](08-session-history.md) | New tool | Fetch session message history |
| 09 | [session_send](09-session-send.md) | New tool | Send into existing sessions |
| 10 | [session_status](10-session-status.md) | New tool | Session status card (tokens, cost) |
| 11 | [sessions_list](11-sessions-list.md) | New tool | List sessions with filters |
| 12 | [sessions_spawn](12-sessions-spawn.md) | New tool | Flexible session spawning |
| 13 | [subagents](13-subagents-management.md) | New tool | List/kill/steer sub-agents |
| 14 | [yield_turn](14-yield.md) | New tool | Pause agent loop for async results |
| 22 | [ask_user](22-ask-user.md) | New tool | Structured questions to user |
| 16 | [config (was gateway)](16-gateway.md) | Expand | Agent self-config (reconciled w/ Claude Code) |
| 20 | [task management](20-task-management.md) | New tool | Task tracking with dependencies |
| 21 | [worktree](21-worktree.md) | New tool | Git worktree isolation |
| 24 | [notebook support](24-notebook-edit.md) | Expand | Jupyter .ipynb in read_file + edit_file |
| 25 | [schedule](25-scheduling.md) | New tool | Manage agent task schedules and pulse |

### Phase 3 — High Complexity (external dependencies)

| # | Plan | Type | Description |
|---|------|------|-------------|
| 05 | [image_analysis](05-image-analysis.md) | New tool | Analyze images via vision LLM |
| 06 | [image_generation](06-image-generation.md) | New tool | Generate images via AI model |
| 07 | [message (channels)](07-message-channels.md) | New tool | Send to Slack/Discord/webhooks |
| 15 | [browser](15-browser.md) | New tool | Headless browser automation |
| 19 | [lsp](19-lsp.md) | New tool | Language Server Protocol code intelligence |
| 23 | [mcp](23-mcp.md) | New tool | Model Context Protocol integration |

### Phase 2.5 — Shared UI Foundation

| # | Plan | Type | Description |
|---|------|------|-------------|
| 26 | [tool-use UI foundation](26-tool-use-ui.md) | Foundation | Shared human-readable tool chip/detail system for current and upcoming tools |

## Running This Roadmap

Do not hand an implementation agent this entire README and say "run everything."

The most efficient workflow is:

1. pick one session from the bundle list below
2. give the agent only the plan files for that session
3. keep the coding session scoped to that bundle's acceptance criteria
4. finish one bundle before starting the next

For tool/UI work, plan [26-tool-use-ui](26-tool-use-ui.md) should land first so later tool plans can plug into the shared presentation layer instead of inventing one-off `ToolUseBlock` UI.

### Session Strategy

- Run one focused coding session at a time
- Do not preload unrelated plan files
- Prefer bundled sessions only where the plans share runtime, DB, or UI infrastructure
- Prefer copy-pasting the prompt for a single session rather than handing over the whole roadmap

### Recommended Session Order

1. [00-refactor-agent-tools](00-refactor-agent-tools.md)
2. [26-tool-use-ui](26-tool-use-ui.md)
3. built-in tool formatter pass on existing tools
4. [03-exec-background](03-exec-background.md)
5. bundle: [01-edit-file](01-edit-file.md) + [17-glob](17-glob.md) + [18-grep](18-grep.md)
6. bundle: [02-web-fetch](02-web-fetch.md) + [05-image-analysis](05-image-analysis.md)
7. bundle: [08-session-history](08-session-history.md) + [10-session-status](10-session-status.md) + [11-sessions-list](11-sessions-list.md)
8. bundle: [09-session-send](09-session-send.md) + [12-sessions-spawn](12-sessions-spawn.md) + [13-subagents-management](13-subagents-management.md)
9. bundle: [14-yield](14-yield.md) + [22-ask-user](22-ask-user.md)
10. bundle: [16-gateway](16-gateway.md) + [21-worktree](21-worktree.md)
11. [20-task-management](20-task-management.md)
12. [25-scheduling](25-scheduling.md)
13. [23-mcp](23-mcp.md)
14. [07-message-channels](07-message-channels.md)
15. [24-notebook-edit](24-notebook-edit.md)
16. [06-image-generation](06-image-generation.md)
17. [15-browser](15-browser.md)
18. [19-lsp](19-lsp.md)

### Why These Bundles

- **File editing/search**: plans 01, 17, and 18 all touch file-navigation and content-search flows
- **Fetch + image analysis**: plans 02 and 05 both benefit from shared fetch/SSRF-safe network helpers
- **Session inspection**: plans 08, 10, and 11 share session-query and read-only inspection logic
- **Session control**: plans 09, 12, and 13 share session creation/execution/control flows
- **Pause/resume**: plans 14 and 22 should share one wait/resume runtime instead of building two
- **Self-management + worktree**: plans 16 and 21 both change execution context / agent self-management behavior

### Coverage Map

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
| 26 | standalone foundation |

## Common Implementation Pattern

> **Prerequisite**: Plan [00-refactor-agent-tools](00-refactor-agent-tools.md) must be completed first. It splits the monolithic `agent_tools.rs` into per-tool modules using a `ToolHandler` trait.

Every **new** tool creates 1 new file and typically touches 4 existing files:

1. **`src-tauri/src/executor/tools/{tool_name}.rs`** (new) — implement `ToolHandler` trait with `name()`, `definition()`, `execute()`
2. **`src-tauri/src/executor/tools/mod.rs`** — add `pub mod {tool_name};` and register in `all_tools()`
3. **`src-tauri/src/executor/permissions.rs`** — risk classification in `classify_tool_call()`
4. **`src/components/chat/toolVisuals.ts`** — Lucide icon + color
5. **`src/screens/AgentInspector/ConfigTab.tsx`** — category + enablement toggle

Some plans also require shared runtime work outside this pattern, especially:
- pause/resume plumbing (`yield_turn`, `ask_user`)
- provider/settings expansion (`image_generation`)
- DB migrations/query helpers (`sessions_*`, `task`)

Detailed `ToolUseBlock` presentation should be treated as a shared foundation, not reimplemented per tool plan. Plan [26-tool-use-ui](26-tool-use-ui.md) should land before the next wave of tool work that needs richer detail panels, and individual tool plans should plug their payloads into that shared system rather than inventing bespoke card layouts.

Tools that **expand** existing tools (03, 17, 24) modify the existing tool file in `tools/` instead of creating a new one.

## New Frontend Tool Categories

- **Sessions**: session_history, session_send, session_status, sessions_list, sessions_spawn
- **Vision**: image_analysis, image_generation
- **Browser**: browser
- **Code Intelligence**: lsp
- **Task Management**: task
- **Scheduling**: schedule
- **Integrations**: mcp

## Already Implemented in Orbit

- `shell_command`, `read_file`, `write_file`, `list_files`
- `web_search` (Brave/Tavily)
- `send_message` (inter-agent bus)
- `spawn_sub_agents`
- `remember`, `forget`, `search_memory`, `list_memories`
- `finish`, `react_to_message`, `activate_skill`

## Full Cross-Reference Matrix

| Capability | Orbit | OpenClaw | Claude Code | Plan |
|-----------|-------|----------|-------------|------|
| Read files | `read_file` | Read | Read | Expand (24) |
| Write files | `write_file` | Write | Write | - |
| Edit files (diff) | - | Edit | Edit | **01** |
| List/glob files | `list_files` | - | Glob | Expand (17) |
| Search content | - | - | Grep | **18** |
| Shell execution | `shell_command` | Exec | Bash | Expand (03) |
| Background exec | - | Exec+Process | Bash bg | Expand (03) |
| Process management | - | Process | - | Expand (03) |
| Web search | `web_search` | Web Search | WebSearch | - |
| Web fetch | - | Web Fetch | WebFetch | **02** |
| Inter-agent messaging | `send_message` | Session Send | SendMessage | - |
| External messaging | - | Message | - | **07** |
| Sub-agent spawn | `spawn_sub_agents` | Sessions spawn | Agent | - |
| Sub-agent management | - | Subagents | - | **13** |
| Session history | - | Session History | - | **08** |
| Session send (existing) | - | Session Send | - | **09** |
| Session status | - | Session Status | - | **10** |
| Session list | - | Sessions list | - | **11** |
| Session spawn (flexible) | - | Sessions spawn | Agent | **12** |
| Yield/pause | - | Yield | - | **14** |
| Image analysis | - | Image | - | **05** |
| Image generation | - | Image Generation | - | **06** |
| Browser automation | - | Browser | - | **15** |
| Self-config | - | Gateway | Config | **16** |
| Memory | 4 tools | - | - | - |
| Skills | `activate_skill` | - | Skill | - |
| Task tracking | - | - | 7 Task tools | **20** |
| Git worktree | - | - | Worktree | **21** |
| Ask user | - | - | AskUserQuestion | **22** |
| MCP integration | - | - | 4 MCP tools | **23** |
| Notebook support | - | - | NotebookEdit | **24** |
| Scheduling / pulse | - | Cron | CronCreate/List | **25** |
| LSP code intel | - | - | LSP | **19** |
| Teams/swarm | - | - | TeamCreate | Future |
| Reactions | `react_to_message` | - | - | - |
| Finish | `finish` | - | - | - |
