# Orbit Streaming Rework for Claude-Code-Level Smoothness

## Summary
- Orbit’s current chat streaming is materially less sophisticated than Claude Code’s stream path. Claude Code keeps live text, thinking, and tool-input state separate from committed transcript messages, renders streaming markdown through a stable-prefix path, and avoids full-message-list churn during every delta.
- Orbit has at least one correctness bug today: when `claude_cli` emits live `tool_result` blocks as `agent:content_block`, the frontend routes them into `addContentBlock`, which only accepts `thinking` and `tool_use`, so those live tool results are dropped. This is visible in [src-tauri/src/executor/claude_cli.rs](/Users/matwaroff/Code/orbit/src-tauri/src/executor/claude_cli.rs:293), [src/App.tsx](/Users/matwaroff/Code/orbit/src/App.tsx:62), and [src/store/liveChatStore.ts](/Users/matwaroff/Code/orbit/src/store/liveChatStore.ts:247).
- Orbit also drops or delays smoothness-critical stream state:
  - no live `thinking_delta` rendering
  - no partial `input_json_delta` / tool-input rendering
  - full markdown reparse on every text delta
  - duplicated streaming logic between chat and run views
  - count-based history reconciliation that can clear the live stream too early

## Implementation Changes
- Replace the current overloaded frontend stream handling with a single shared streaming reducer used by both chat sessions and live run views.
  - Move stream state out of `displayMessages` mutation-per-delta and into explicit preview state: `streamingText`, `streamingThinking`, `streamingToolUses`, `inProgressToolResults`, and committed `messages`.
  - Use the shared reducer from both `liveChatStore` and `liveRunStore` so chat and run views stop drifting.
- Normalize backend stream events across `anthropic`, `minimax`, and `claude_cli`.
  - Keep `agent:llm_chunk` for text deltas.
  - Introduce explicit delta/final event contracts instead of overloading `AgentContentBlockPayload.block: ContentBlock`.
  - Add dedicated frontend/backend payloads for:
    - live thinking delta
    - live tool-input delta
    - finalized content blocks
    - tool results
  - Make `claude_cli` emit tool results through the same tool-result event path as other providers instead of encoding them as generic content blocks.
  - Make `anthropic` and `minimax` emit live thinking deltas and live tool-input deltas, not only finalized blocks on `content_block_stop`.
- Rework chat rendering so the virtualized transcript is mostly stable during streaming.
  - Render the in-progress assistant reply outside the main virtualized message list as a dedicated preview row.
  - Commit the final assistant message into transcript state atomically when the turn/block finalizes.
  - Replace count-based stream/history handoff in `ChatPanel` with identity-based reconciliation tied to the sent user message id and authoritative persisted assistant data.
- Add a web `StreamingMarkdown` path modeled after Claude Code’s behavior.
  - Split stable prefix from unstable suffix so only the growing tail reparses while streaming.
  - Render only completed lines/blocks in the preview to reduce visual jitter.
  - Keep the current full `ReactMarkdown` path for committed transcript messages.
- Improve live thinking and tool UX.
  - Show thinking progressively while streaming, then collapse it into the final assistant message once committed.
  - Show partial tool-use cards as the input JSON streams in, then replace them with finalized tool calls/results.
  - Keep tool results attached to their originating tool-use in both live and persisted views.
- Relax composer behavior during streaming.
  - Do not disable text entry while a response is in flight; only prevent a second send while the current turn is active.
  - Keep stop/cancel available without freezing draft editing.

## Public Interfaces / Type Changes
- Replace the current internal Tauri event contract around `AgentContentBlockPayload` with discriminated stream payloads.
  - `AgentContentBlockPayload` should no longer claim `block: ContentBlock` for delta-only events.
  - Add explicit payload types for `thinking_delta` and `tool_input_delta`, plus a finalized content-block payload.
- Keep `send_chat_message` callable shape unchanged.
- Reuse the existing `userMessageId` from `SendChatMessageResponse` as the anchor for stream-to-history reconciliation.

## Test Plan
- Provider parity:
  - Anthropic stream shows live text, live thinking, partial tool input, finalized tool result.
  - MiniMax matches the same behavior.
  - `claude_cli` no longer drops live tool results or live thinking.
- UI smoothness:
  - long markdown reply does not reparse the full transcript per delta
  - auto-scroll stays pinned near bottom without jitter
  - final message appears in the same frame the preview disappears
  - typing into the composer remains responsive during streaming
- State correctness:
  - cancel mid-stream preserves partial assistant output cleanly
  - tool result always attaches to the correct tool-use
  - chat and run views behave the same for the same event sequence
  - persisted history never replaces/clears the live stream until the finished turn is actually present in DB-backed query data
- Regression coverage:
  - unit tests for the shared reducer using recorded event sequences
  - component tests for preview-to-final handoff
  - one integration test per provider event shape

## Assumptions
- Scope includes both user chat sessions and the live run inspector, not just one surface.
- Scope includes all currently supported streaming providers in Orbit, especially `anthropic`, `minimax`, and `claude_cli`.
- The goal is behavioral parity with Claude Code’s smoothness patterns, not a pixel-identical terminal-style presentation.
