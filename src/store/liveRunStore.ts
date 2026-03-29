import { create } from "zustand";
import { LogLine, RunState, RunSummary, ContentBlock } from "../types";
import { DisplayMessage, DisplayBlock } from "../components/chat/types";
import { info } from "@tauri-apps/plugin-log";

interface AgentLoopState {
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
  // Agent loop state tracking
  updateAgentIteration: (runId: string, iteration: number, action: string, totalTokens: number) => void;
  appendLlmChunk: (runId: string, delta: string, iteration: number) => void;
  // Structured message tracking for ChatView
  appendTextDelta: (runId: string, delta: string, iteration: number) => void;
  addContentBlock: (runId: string, block: ContentBlock, iteration: number) => void;
  addToolResult: (runId: string, toolUseId: string, content: string, isError: boolean) => void;
  handleIteration: (runId: string, iteration: number, action: string, totalTokens: number) => void;
}

const TERMINAL_STATES: RunState[] = [
  "success", "failure", "cancelled", "timed_out",
];

function ensureAgentState(run: LiveRun): AgentLoopState {
  return run.agentLoopState ?? {
    iteration: 0,
    totalTokens: 0,
    currentAction: "llm_call",
    llmStreamBuffer: "",
    displayMessages: [],
  };
}

/**
 * Get or create the current assistant message for streaming content into.
 * Returns the updated displayMessages array and index of the current assistant message.
 */
function getOrCreateAssistantMessage(
  msgs: DisplayMessage[]
): [DisplayMessage[], number] {
  const last = msgs[msgs.length - 1];
  if (last && last.role === "assistant" && last.isStreaming) {
    return [msgs, msgs.length - 1];
  }
  // Create a new streaming assistant message
  const newMsg: DisplayMessage = {
    id: nextMsgId(),
    role: "assistant",
    blocks: [],
    isStreaming: true,
  };
  return [[...msgs, newMsg], msgs.length];
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

  // ── Structured message tracking ──────────────────────────────────────

  appendTextDelta: (runId, delta, iteration) =>
    set((state) => {
      const run = state.activeRuns[runId];
      if (!run) return state;
      const prev = ensureAgentState(run);
      const [msgs, idx] = getOrCreateAssistantMessage([...prev.displayMessages]);
      const msg = { ...msgs[idx] };
      const blocks = [...msg.blocks];

      // Append to existing streaming text block, or create one
      const lastBlock = blocks[blocks.length - 1];
      if (lastBlock && lastBlock.kind === "text" && lastBlock.isStreaming) {
        blocks[blocks.length - 1] = {
          ...lastBlock,
          text: lastBlock.text + delta,
        };
      } else {
        blocks.push({ kind: "text", text: delta, isStreaming: true });
      }

      msg.blocks = blocks;
      msgs[idx] = msg;

      return {
        activeRuns: {
          ...state.activeRuns,
          [runId]: {
            ...run,
            agentLoopState: {
              ...prev,
              iteration,
              llmStreamBuffer: prev.llmStreamBuffer + delta,
              displayMessages: msgs,
            },
          },
        },
      };
    }),

  addContentBlock: (runId, block, iteration) =>
    set((state) => {
      const run = state.activeRuns[runId];
      if (!run) return state;
      const prev = ensureAgentState(run);
      const [msgs, idx] = getOrCreateAssistantMessage([...prev.displayMessages]);
      const msg = { ...msgs[idx] };
      const blocks = [...msg.blocks];

      // Finalize any streaming text block
      const lastBlock = blocks[blocks.length - 1];
      if (lastBlock && lastBlock.kind === "text" && lastBlock.isStreaming) {
        blocks[blocks.length - 1] = { ...lastBlock, isStreaming: false };
      }

      // Add the content block
      let displayBlock: DisplayBlock;
      if (block.type === "thinking") {
        displayBlock = { kind: "thinking", thinking: block.thinking };
      } else if (block.type === "tool_use") {
        displayBlock = {
          kind: "tool_call",
          id: block.id,
          name: block.name,
          input: block.input,
        };
      } else {
        // Shouldn't normally happen, but handle gracefully
        return state;
      }

      blocks.push(displayBlock);
      msg.blocks = blocks;
      msgs[idx] = msg;

      return {
        activeRuns: {
          ...state.activeRuns,
          [runId]: {
            ...run,
            agentLoopState: {
              ...prev,
              iteration,
              displayMessages: msgs,
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
      const msgs = [...prev.displayMessages];

      // Find the tool_call block with matching id and attach the result
      for (let i = msgs.length - 1; i >= 0; i--) {
        const msg = msgs[i];
        if (msg.role !== "assistant") continue;
        for (let j = msg.blocks.length - 1; j >= 0; j--) {
          const block = msg.blocks[j];
          if (block.kind === "tool_call" && block.id === toolUseId) {
            const updatedBlocks = [...msg.blocks];
            updatedBlocks[j] = { ...block, result: { content, isError } };
            msgs[i] = { ...msg, blocks: updatedBlocks };

            return {
              activeRuns: {
                ...state.activeRuns,
                [runId]: {
                  ...run,
                  agentLoopState: {
                    ...prev,
                    displayMessages: msgs,
                  },
                },
              },
            };
          }
        }
      }

      return state;
    }),

  handleIteration: (runId, iteration, action, totalTokens) =>
    set((state) => {
      const run = state.activeRuns[runId];
      if (!run) return state;
      const prev = ensureAgentState(run);
      const msgs = [...prev.displayMessages];

      // When a new llm_call starts, finalize the previous assistant message
      if (action === "llm_call" && msgs.length > 0) {
        const last = msgs[msgs.length - 1];
        if (last.role === "assistant" && last.isStreaming) {
          const blocks = [...last.blocks];
          const lastBlock = blocks[blocks.length - 1];
          if (lastBlock && lastBlock.kind === "text" && lastBlock.isStreaming) {
            blocks[blocks.length - 1] = { ...lastBlock, isStreaming: false };
          }
          msgs[msgs.length - 1] = { ...last, blocks, isStreaming: false };
        }
      }

      // When finished, mark everything as not streaming
      if (action === "finished" && msgs.length > 0) {
        const last = msgs[msgs.length - 1];
        if (last.isStreaming) {
          const blocks = [...last.blocks];
          const lastBlock = blocks[blocks.length - 1];
          if (lastBlock && lastBlock.kind === "text" && lastBlock.isStreaming) {
            blocks[blocks.length - 1] = { ...lastBlock, isStreaming: false };
          }
          msgs[msgs.length - 1] = { ...last, blocks, isStreaming: false };
        }
      }

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
              displayMessages: msgs,
            },
          },
        },
      };
    }),
}));
