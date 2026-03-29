import { listen } from "@tauri-apps/api/event";
import {
  RunLogChunkPayload,
  RunStateChangedPayload,
  AgentLlmChunkPayload,
  AgentIterationPayload,
  AgentContentBlockPayload,
  AgentToolResultPayload,
} from "../types";

export function onRunLogChunk(
  handler: (payload: RunLogChunkPayload) => void
) {
  return listen<RunLogChunkPayload>("run:log_chunk", (event) => {
    handler(event.payload);
  });
}

export function onRunStateChanged(
  handler: (payload: RunStateChangedPayload) => void
) {
  return listen<RunStateChangedPayload>("run:state_changed", (event) => {
    handler(event.payload);
  });
}

export function onAgentLlmChunk(
  handler: (payload: AgentLlmChunkPayload) => void
) {
  return listen<AgentLlmChunkPayload>("agent:llm_chunk", (event) => {
    handler(event.payload);
  });
}

export function onAgentIteration(
  handler: (payload: AgentIterationPayload) => void
) {
  return listen<AgentIterationPayload>("agent:iteration", (event) => {
    handler(event.payload);
  });
}

export function onAgentContentBlock(
  handler: (payload: AgentContentBlockPayload) => void
) {
  return listen<AgentContentBlockPayload>("agent:content_block", (event) => {
    handler(event.payload);
  });
}

export function onAgentToolResult(
  handler: (payload: AgentToolResultPayload) => void
) {
  return listen<AgentToolResultPayload>("agent:tool_result", (event) => {
    handler(event.payload);
  });
}
