import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, Square, Cpu, Wrench, CheckCircle } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { agentsApi } from "../../api/agents";
import { useLiveRunStore } from "../../store/liveRunStore";
import { onRunLogChunk, onRunStateChanged } from "../../events/runEvents";
import { Run, ChatMessage } from "../../types";
import { ChatView } from "../../components/chat";

interface AgentRunViewProps {
  runId: string;
  onBack: () => void;
}

const TERMINAL_STATES = ["success", "failure", "cancelled", "timed_out"];

export function AgentRunView({ runId, onBack }: AgentRunViewProps) {
  const store = useLiveRunStore();
  const liveRun = store.activeRuns[runId];

  const { data: run } = useQuery<Run>({
    queryKey: ["run", runId],
    queryFn: () => invoke("get_run", { id: runId }),
    refetchInterval: liveRun ? 3_000 : false,
  });

  const isActive = liveRun && !TERMINAL_STATES.includes(liveRun.state);

  // Fetch conversation history for completed runs
  const { data: conversation } = useQuery<ChatMessage[] | null>({
    queryKey: ["agent-conversation", runId],
    queryFn: () => invoke("get_agent_conversation", { runId }),
    enabled: !isActive,
  });

  // Subscribe to run state & log events (still needed for state tracking)
  useEffect(() => {
    const unsubs: Promise<() => void>[] = [];

    unsubs.push(
      onRunLogChunk((payload) => {
        if (payload.runId === runId) {
          store.appendLogChunk(runId, payload.lines);
        }
      })
    );

    unsubs.push(
      onRunStateChanged((payload) => {
        if (payload.runId === runId) {
          store.updateRunState(runId, payload.newState);
        }
      })
    );

    return () => {
      unsubs.forEach((p) => p.then((unsub) => unsub()));
    };
  }, [runId]);

  const agentState = liveRun?.agentLoopState;
  const metadata = run?.metadata as Record<string, unknown> | undefined;
  const loopMeta = metadata?.agent_loop as Record<string, number> | undefined;

  const iteration = agentState?.iteration ?? loopMeta?.iteration ?? 0;
  const totalTokens = agentState?.totalTokens ?? loopMeta?.total_tokens ?? 0;

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="flex items-center gap-3 px-4 py-3 border-b border-edge">
        <button
          onClick={onBack}
          className="p-1.5 rounded text-muted hover:text-white hover:bg-edge"
        >
          <ArrowLeft size={16} />
        </button>
        <div className="flex-1 min-w-0">
          <p className="text-sm font-semibold text-white truncate">
            Agent Run {runId.slice(0, 8)}...
          </p>
          <p className="text-xs text-muted">
            {run?.state ?? liveRun?.state ?? "pending"}
          </p>
        </div>

        {/* Stats badges */}
        <div className="flex items-center gap-3 text-xs">
          <div className="flex items-center gap-1 text-accent-hover">
            <Cpu size={12} />
            <span>Iter {iteration}</span>
          </div>
          <div className="flex items-center gap-1 text-muted">
            <span>{totalTokens.toLocaleString()} tokens</span>
          </div>
          {agentState?.currentAction && (
            <div className="flex items-center gap-1">
              {agentState.currentAction === "llm_call" && (
                <Cpu size={12} className="text-blue-400 animate-pulse" />
              )}
              {agentState.currentAction === "tool_exec" && (
                <Wrench size={12} className="text-amber-400" />
              )}
              {agentState.currentAction === "finished" && (
                <CheckCircle size={12} className="text-emerald-400" />
              )}
              <span className="text-secondary">{agentState.currentAction}</span>
            </div>
          )}
        </div>

        {isActive && (
          <button
            onClick={() => agentsApi.cancelRun(runId)}
            className="flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-xs text-red-400 hover:bg-red-500/10 border border-red-500/30"
          >
            <Square size={11} /> Stop
          </button>
        )}
      </div>

      {/* Chat view */}
      <ChatView
        liveRunId={isActive ? runId : undefined}
        messages={!isActive && conversation ? conversation : undefined}
        className="flex-1 min-h-0"
      />
    </div>
  );
}
