import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, Square, Cpu, Wrench, CheckCircle } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { agentsApi } from "../../api/agents";
import { useLiveRunStore } from "../../store/liveRunStore";
import { onAgentLlmChunk, onAgentIteration, onRunLogChunk, onRunStateChanged } from "../../events/runEvents";
import { Run, LogLine } from "../../types";

interface AgentRunViewProps {
  runId: string;
  onBack: () => void;
}

export function AgentRunView({ runId, onBack }: AgentRunViewProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const store = useLiveRunStore();
  const liveRun = store.activeRuns[runId];

  const { data: run } = useQuery<Run>({
    queryKey: ["run", runId],
    queryFn: () => invoke("get_run", { id: runId }),
    refetchInterval: liveRun ? 3_000 : false,
  });

  // Subscribe to events
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

    unsubs.push(
      onAgentLlmChunk((payload) => {
        if (payload.runId === runId) {
          store.appendLlmChunk(runId, payload.delta, payload.iteration);
        }
      })
    );

    unsubs.push(
      onAgentIteration((payload) => {
        if (payload.runId === runId) {
          store.updateAgentIteration(
            runId,
            payload.iteration,
            payload.action,
            payload.totalTokens
          );
        }
      })
    );

    return () => {
      unsubs.forEach((p) => p.then((unsub) => unsub()));
    };
  }, [runId]);

  // Auto-scroll
  useEffect(() => {
    if (autoScroll && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [liveRun?.logs.length, liveRun?.agentLoopState?.llmStreamBuffer, autoScroll]);

  function handleScroll() {
    if (!scrollRef.current) return;
    const { scrollTop, scrollHeight, clientHeight } = scrollRef.current;
    setAutoScroll(scrollHeight - scrollTop - clientHeight < 50);
  }

  const agentState = liveRun?.agentLoopState;
  const isActive = liveRun && !["success", "failure", "cancelled", "timed_out"].includes(liveRun.state);
  const metadata = run?.metadata as Record<string, unknown> | undefined;
  const loopMeta = metadata?.agent_loop as Record<string, number> | undefined;

  const iteration = agentState?.iteration ?? loopMeta?.iteration ?? 0;
  const totalTokens = agentState?.totalTokens ?? loopMeta?.total_tokens ?? 0;

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="flex items-center gap-3 px-4 py-3 border-b border-[#2a2d3e]">
        <button
          onClick={onBack}
          className="p-1.5 rounded text-[#64748b] hover:text-white hover:bg-[#2a2d3e]"
        >
          <ArrowLeft size={16} />
        </button>
        <div className="flex-1 min-w-0">
          <p className="text-sm font-semibold text-white truncate">
            Agent Run {runId.slice(0, 8)}...
          </p>
          <p className="text-xs text-[#64748b]">
            {run?.state ?? liveRun?.state ?? "pending"}
          </p>
        </div>

        {/* Stats badges */}
        <div className="flex items-center gap-3 text-xs">
          <div className="flex items-center gap-1 text-[#818cf8]">
            <Cpu size={12} />
            <span>Iter {iteration}</span>
          </div>
          <div className="flex items-center gap-1 text-[#64748b]">
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
              <span className="text-[#94a3b8]">{agentState.currentAction}</span>
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

      {/* Log output */}
      <div
        ref={scrollRef}
        onScroll={handleScroll}
        className="flex-1 overflow-y-auto p-4 bg-[#0a0c12] font-mono text-xs leading-relaxed"
      >
        {liveRun?.logs.map((line: LogLine, i: number) => (
          <div
            key={i}
            className={
              line.stream === "stderr" ? "text-red-400" : "text-[#e2e8f0]"
            }
          >
            {line.line}
          </div>
        ))}
        {!liveRun?.logs.length && !isActive && (
          <div className="text-[#64748b]">
            {run?.state === "success"
              ? "Run completed. View full log in Run History."
              : run?.state === "failure"
              ? "Run failed."
              : "Waiting for output..."}
          </div>
        )}
      </div>
    </div>
  );
}
