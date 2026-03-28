import { listen } from "@tauri-apps/api/event";
import { RunLogChunkPayload, RunStateChangedPayload } from "../types";

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
