import { create } from "zustand";
import { LogLine, RunState, RunSummary } from "../types";
import { info } from "@tauri-apps/plugin-log";

interface LiveRun {
  runId: string;
  taskName: string;
  state: RunState;
  startedAt: string | null;
  logs: LogLine[];
}

interface LiveRunStore {
  activeRuns: Record<string, LiveRun>;
  // Called when a run starts or its state changes
  upsertRun: (summary: RunSummary) => void;
  // Called when a run:state_changed event fires
  updateRunState: (runId: string, newState: RunState) => void;
  // Called when log chunks arrive
  appendLogChunk: (runId: string, lines: LogLine[]) => void;
  // Remove a run from active tracking once it reaches a terminal state
  removeRun: (runId: string) => void;
  clearLogs: (runId: string) => void;
}

const TERMINAL_STATES: RunState[] = [
  "success", "failure", "cancelled", "timed_out",
];

export const useLiveRunStore = create<LiveRunStore>((set) => ({
  activeRuns: {},

  upsertRun: (summary) =>
    set((state) => ({
      activeRuns: {
        ...state.activeRuns,
        [summary.id]: {
          runId: summary.id,
          taskName: summary.taskName,
          state: summary.state,
          startedAt: summary.startedAt,
          logs: state.activeRuns[summary.id]?.logs ?? [],
        },
      },
    })),

  updateRunState: (runId, newState) =>
    set((state) => {
      const run = state.activeRuns[runId];

      // If run doesn't exist yet (e.g., manually triggered task), create a placeholder
      // so subsequent log chunks can be stored. This ensures manual triggers work
      // even when the Dashboard isn't mounted to call upsertRun first.
      if (!run) {
        info(`Manually triggered run ${runId}`);
        const placeholder: LiveRun = {
          runId,
          taskName: "Unknown Task",
          state: newState,
          startedAt: null,
          logs: [],
        };
        const updated = { ...state.activeRuns, [runId]: placeholder };

        if (TERMINAL_STATES.includes(newState)) {
          setTimeout(() => {
            useLiveRunStore.getState().removeRun(runId);
          }, 5000);
        }

        return { activeRuns: updated };
      }

      const updated = { ...state.activeRuns, [runId]: { ...run, state: newState } };

      // Keep terminal runs briefly for UI feedback then remove
      if (TERMINAL_STATES.includes(newState)) {
        setTimeout(() => {
          useLiveRunStore.getState().removeRun(runId);
        }, 5000);
      }

      return { activeRuns: updated };
    }),

  appendLogChunk: (runId, lines) =>
    set((state) => {
      const run = state.activeRuns[runId];
      if (!run) return state;
      return {
        activeRuns: {
          ...state.activeRuns,
          [runId]: { ...run, logs: [...run.logs, ...lines] },
        },
      };
    }),

  removeRun: (runId) =>
    set((state) => {
      const { [runId]: _, ...rest } = state.activeRuns;
      return { activeRuns: rest };
    }),

  clearLogs: (runId) =>
    set((state) => {
      const run = state.activeRuns[runId];
      if (!run) return state;
      return {
        activeRuns: { ...state.activeRuns, [runId]: { ...run, logs: [] } },
      };
    }),
}));
