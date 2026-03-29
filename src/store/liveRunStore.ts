import { create } from "zustand";
import { LogLine, RunState, RunSummary } from "../types";
import { info } from "@tauri-apps/plugin-log";

interface AgentLoopState {
  iteration: number;
  totalTokens: number;
  currentAction: string;
  llmStreamBuffer: string;
}

interface LiveRun {
  runId: string;
  taskName: string;
  state: RunState;
  startedAt: string | null;
  logs: LogLine[];
  agentLoopState?: AgentLoopState;
}

interface LiveRunStore {
  activeRuns: Record<string, LiveRun>;
  upsertRun: (summary: RunSummary) => void;
  updateRunState: (runId: string, newState: RunState) => void;
  appendLogChunk: (runId: string, lines: LogLine[]) => void;
  removeRun: (runId: string) => void;
  clearLogs: (runId: string) => void;
  // Agent loop state tracking
  updateAgentIteration: (runId: string, iteration: number, action: string, totalTokens: number) => void;
  appendLlmChunk: (runId: string, delta: string, iteration: number) => void;
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

  updateAgentIteration: (runId, iteration, action, totalTokens) =>
    set((state) => {
      const run = state.activeRuns[runId];
      if (!run) return state;
      return {
        activeRuns: {
          ...state.activeRuns,
          [runId]: {
            ...run,
            agentLoopState: {
              ...(run.agentLoopState ?? { llmStreamBuffer: "" }),
              iteration,
              currentAction: action,
              totalTokens,
            },
          },
        },
      };
    }),

  appendLlmChunk: (runId, delta, iteration) =>
    set((state) => {
      const run = state.activeRuns[runId];
      if (!run) return state;
      const prev = run.agentLoopState ?? {
        iteration: 0,
        totalTokens: 0,
        currentAction: "llm_call",
        llmStreamBuffer: "",
      };
      return {
        activeRuns: {
          ...state.activeRuns,
          [runId]: {
            ...run,
            agentLoopState: {
              ...prev,
              iteration,
              llmStreamBuffer: prev.llmStreamBuffer + delta,
            },
          },
        },
      };
    }),
}));
