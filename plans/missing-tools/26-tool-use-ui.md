# Plan: Unified Tool-Use UI Foundation

> **Foundation-first**: This plan should land before the next wave of tool implementations so new tools can plug into a shared presentation system instead of inventing one-off `ToolUseBlock` UI.

## Context

Orbit's chat currently renders tool inputs as raw JSON and tool results as raw strings. That is workable for debugging but poor for day-to-day use, especially as more tools land and start returning richer structured output.

Several missing-tool plans already call for custom tool UI in `ToolUseBlock.tsx`. If each plan ships its own one-off card layout, the chat UI will drift into inconsistent patterns and duplicate formatting logic. This plan centralizes the shared rendering framework first, then lets individual tool plans add tool-specific formatters against that framework as their payloads stabilize.

## Sequencing

This plan should be implemented **before** the next batch of tool work that depends on richer tool-detail UI.

The shared framework portion should cover:
- already-shipped tools and fallbacks for unknown future tools
- debug / verbose mode
- normalized result parsing for common existing result shapes
- chip expandability rules and shared panel styling

Then, as individual tool plans land, they should plug tool-specific renderers into this foundation instead of building bespoke detail panels from scratch.

## Covered Tools

### Existing Orbit tools

- `shell_command`
- `read_file`
- `write_file`
- `list_files`
- `web_search`
- `send_message`
- `spawn_sub_agents`
- `remember`
- `forget`
- `search_memory`
- `list_memories`
- `finish`
- `activate_skill`

### Planned / expanded tools

- `browser`
- `image_generation`
- `yield_turn`
- `ask_user`
- `task`
- `schedule`
- `mcp`
- notebook-aware `read_file`
- notebook-aware `edit_file`

### Optional to fold in during the same pass if already implemented

- `subagents`
- `web_fetch`
- `session_history`
- `session_send`
- `session_status`
- `sessions_list`
- `sessions_spawn`
- `grep`
- `lsp`
- `worktree`

## What Changes

### Shared presentation layer

Build a formatter/presentation registry for tool calls used by `src/components/chat/ToolUseBlock.tsx`:

- Normalize raw tool result strings before rendering
- Strip stored `<tool_result ...>...</tool_result>` wrappers for persisted tool results
- Parse common result patterns into structured sections rather than rendering one big `pre`
- Keep the raw payload available for debug mode
- Provide a generic fallback renderer so new tools remain usable before they get a dedicated formatter

### Global settings

Extend chat display settings with a new global toggle for verbose tool details:

- Default: off
- Off: expanded tool chips show only the human-readable presentation
- On: expanded chips additionally show raw input JSON and raw result text

### Expandability policy

Balanced default behavior:

- Rich expandable panels for inspectable tools such as shell/file/search/memory/browser/task/schedule/MCP
- Chip-only by default for status/action tools such as `remember`, `forget`, `finish`, and `activate_skill`
- Existing interactive pending states that are required for execution, such as `ask_user` and `yield_turn`, remain in scope and should plug into the shared styling system instead of bypassing it

## Delivery Strategy

### Phase A — Shared foundation first

Ship these pieces before or alongside the next tool implementations:

- formatter/presentation registry and generic fallback renderer
- raw-result normalization
- verbose/debug setting
- shared tool detail sections and styling
- initial coverage for existing built-in tools that are already noisy today

### Phase B — Tool-specific adapters as features land

As each tool or tool expansion lands, add or refine its dedicated formatter within this shared system:

- new tools should target the shared presentation API from day one
- existing tools can gain richer cards incrementally without reworking the base UI structure
- any tool that returns a brand-new payload shape should add its formatter as part of that tool plan

## Per-Tool Presentation Targets

### Shell / file / search / memory

- `shell_command`: command snippet, timeout/background metadata, structured stdout/stderr/exit status, process cards for background actions
- `read_file`: path-focused input summary and readable file content; notebook output becomes cell-based rendering rather than raw JSON
- `write_file`: path + content summary in normal mode; raw file body only in verbose mode
- `list_files`: directory/table view instead of monospaced listing dump
- `web_search`: query summary plus result cards
- `search_memory` / `list_memories`: memory cards with type badge, text, timestamp

### Agent control / coordination

- `spawn_sub_agents`: preserve and restyle the existing tracker; replace raw task JSON with a readable task summary
- `send_message`: show target agent, wait mode, and concise status/result
- `yield_turn`: show waiting state, wait reason, timeout, and resume outcome in the shared card style
- `ask_user`: preserve interactive inline question UI while aligning surrounding tool chrome with the shared tool panel system
- `task`: render create/list/update/get/delete output as a compact task board or task rows
- `schedule`: render schedules, previews, and pulse state as readable cards/tables

### Rich media / integrations

- `browser`: screenshot preview, action-specific metadata, content/evaluate output sections
- `image_generation`: saved-image preview and output path card
- `mcp`: server/resource/tool call views with action-specific sections instead of raw nested JSON

## Frontend Changes

- `src/components/chat/ToolUseBlock.tsx`: refactor around formatter/presentation helpers instead of direct `JSON.stringify` / raw `<pre>` output
- Add one or more chat formatting helpers/components for structured tool sections
- Reuse existing `TextBlock`/markdown rendering where results are prose-like instead of forcing monospaced output
- Keep `toolVisuals.ts` and tool enablement wiring per individual tool plans; this plan only consolidates how expanded tool details render

## Assumptions

- The foundation is primarily frontend work; backend schema changes should only happen if a later tool proves impossible to present sanely from existing payloads
- Individual tool plans should rely on this foundation and add tool-specific adapters or lightweight metadata as needed rather than inventing bespoke panel layouts
- Unknown future tools should fall back to a generic human-readable renderer plus raw debug mode
