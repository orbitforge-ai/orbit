import { create } from 'zustand';
import { AgentContentBlockPayload, AgentIterationPayload, LogLine, RunState, RunSummary } from '../types';
import { DisplayMessage } from '../components/chat/types';
import { info } from '@tauri-apps/plugin-log';
import {
  appendTextDelta as appendPreviewTextDelta,
  applyContentBlock,
  applyToolResult,
  commitPreviewMessage,
  createEmptyPreviewState,
  StreamPreviewState,
} from './streaming/streamReducer';

interface AgentLoopState extends StreamPreviewState {
  iteration: number;
  totalTokens: number;
  currentAction: string;
  llmStreamBuffer: string;
  displayMessages: DisplayMessage[];
}

interface LiveRun {
  runId: string;
  taskName: string;
  state: RunState;
  startedAt: string | null;
  logs: LogLine[];
  agentLoopState?: AgentLoopState;
}

let msgIdCounter = 0;
function nextMsgId(): string {
  return `live-${++msgIdCounter}`;
}

interface LiveRunStore {
  activeRuns: Record<string, LiveRun>;
  upsertRun: (summary: RunSummary) => void;
  updateRunState: (runId: string, newState: RunState) => void;
  appendLogChunk: (runId: string, lines: LogLine[]) => void;
  removeRun: (runId: string) => void;
  clearLogs: (runId: string) => void;
  updateAgentIteration: (
    runId: string,
    iteration: number,
    action: string,
    totalTokens: number
  ) => void;
  appendLlmChunk: (runId: string, delta: string, iteration: number) => void;
  appendTextDelta: (runId: string, delta: string, iteration: number) => void;
  addContentBlock: (runId: string, payload: AgentContentBlockPayload) => void;
  addToolResult: (runId: string, toolUseId: string, content: string, isError: boolean) => void;
  handleIteration: (runId: string, payload: AgentIterationPayload) => void;
}

const TERMINAL_STATES: RunState[] = ['success', 'failure', 'cancelled', 'timed_out'];

function ensureAgentState(run: LiveRun): AgentLoopState {
  return (
    run.agentLoopState ?? {
      iteration: 0,
      totalTokens: 0,
      currentAction: 'llm_call',
      llmStreamBuffer: '',
      displayMessages: [],
      ...createEmptyPreviewState(),
    }
  );
}

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

      if (!run) {
        info(`Manually triggered run ${runId}`);
        const placeholder: LiveRun = {
          runId,
          taskName: 'Unknown Task',
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
      const { [runId]: _removed, ...rest } = state.activeRuns;
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
      const prev = ensureAgentState(run);
      return {
        activeRuns: {
          ...state.activeRuns,
          [runId]: {
            ...run,
            agentLoopState: {
              ...prev,
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
      const prev = ensureAgentState(run);
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

  appendTextDelta: (runId, delta, iteration) =>
    set((state) => {
      const run = state.activeRuns[runId];
      if (!run) return state;
      const prev = ensureAgentState(run);
      const previewState = appendPreviewTextDelta(prev, delta, nextMsgId);

      return {
        activeRuns: {
          ...state.activeRuns,
          [runId]: {
            ...run,
            agentLoopState: {
              ...prev,
              ...previewState,
              iteration,
              llmStreamBuffer: prev.llmStreamBuffer + delta,
            },
          },
        },
      };
    }),

  addContentBlock: (runId, payload) =>
    set((state) => {
      const run = state.activeRuns[runId];
      if (!run) return state;
      const prev = ensureAgentState(run);
      const previewState = applyContentBlock(prev, payload, nextMsgId);

      return {
        activeRuns: {
          ...state.activeRuns,
          [runId]: {
            ...run,
            agentLoopState: {
              ...prev,
              ...previewState,
              iteration: payload.iteration,
            },
          },
        },
      };
    }),

  addToolResult: (runId, toolUseId, content, isError) =>
    set((state) => {
      const run = state.activeRuns[runId];
      if (!run) return state;
      const prev = ensureAgentState(run);
      const applied = applyToolResult(prev.displayMessages, prev, toolUseId, content, isError);

      return {
        activeRuns: {
          ...state.activeRuns,
          [runId]: {
            ...run,
            agentLoopState: {
              ...prev,
              displayMessages: applied.messages,
              ...applied.state,
            },
          },
        },
      };
    }),

  handleIteration: (runId, payload) =>
    set((state) => {
      const run = state.activeRuns[runId];
      if (!run) return state;
      const prev = ensureAgentState(run);

      if (payload.action === 'llm_call') {
        const committed = commitPreviewMessage(prev.displayMessages, prev, null, nextMsgId);
        return {
          activeRuns: {
            ...state.activeRuns,
            [runId]: {
              ...run,
              agentLoopState: {
                ...prev,
                ...committed.state,
                displayMessages: committed.messages,
                iteration: payload.iteration,
                currentAction: payload.action,
                totalTokens: payload.totalTokens,
              },
            },
          },
        };
      }

      if (payload.action === 'finished') {
        const committed = commitPreviewMessage(
          prev.displayMessages,
          prev,
          payload.finishSummary,
          nextMsgId
        );
        return {
          activeRuns: {
            ...state.activeRuns,
            [runId]: {
              ...run,
              agentLoopState: {
                ...prev,
                ...committed.state,
                displayMessages: committed.messages,
                iteration: payload.iteration,
                currentAction: payload.action,
                totalTokens: payload.totalTokens,
              },
            },
          },
        };
      }

      return {
        activeRuns: {
          ...state.activeRuns,
          [runId]: {
            ...run,
            agentLoopState: {
              ...prev,
              iteration: payload.iteration,
              currentAction: payload.action,
              totalTokens: payload.totalTokens,
            },
          },
        },
      };
    }),
}));
