import { useEffect, useState } from 'react';
import { ChevronRight, CheckCircle, XCircle, Hammer, Loader2, GitBranch, ChevronDown } from 'lucide-react';
import { onRunStateChanged } from '../../events/runEvents';
import { runsApi } from '../../api/runs';
import { RunSummary } from '../../types';

interface ToolUseBlockProps {
  name: string;
  input: Record<string, unknown>;
  result?: { content: string; isError: boolean };
}

export function ToolUseBlock({ name, input, result }: ToolUseBlockProps) {
  const [expanded, setExpanded] = useState(false);
  const inputStr = JSON.stringify(input, null, 2);
  const isSubAgentTool = name === 'spawn_sub_agents';

  return (
    <div className="rounded-lg border border-warning/30 bg-warning/5 overflow-hidden">
      {/* Collapsed header — always visible */}
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-2 px-3 py-2 w-full text-left hover:bg-warning/10 transition-colors"
      >
        <ChevronRight
          size={12}
          className={`text-warning shrink-0 transition-transform ${expanded ? 'rotate-90' : ''}`}
        />
        <Hammer size={12} className="text-warning shrink-0" />
        <span className="text-xs text-muted">Tool Used</span>
        <span className="text-xs font-medium text-warning">{name}</span>
        {result && !result.isError && (
          <CheckCircle size={11} className="text-emerald-400 ml-auto shrink-0" />
        )}
        {result && result.isError && (
          <XCircle size={11} className="text-red-400 ml-auto shrink-0" />
        )}
        {!result && isSubAgentTool && (
          <Loader2 size={11} className="text-accent-hover ml-auto shrink-0 animate-spin" />
        )}
      </button>

      {/* Sub-agent live tracker — shown while waiting for result */}
      {isSubAgentTool && !result && (
        <SubAgentTracker tasks={input.tasks as SubAgentTask[] | undefined} />
      )}

      {/* Expanded details */}
      {expanded && (
        <>
          {/* Input */}
          <div className="border-t border-warning/10">
            <div className="px-3 py-1.5 text-[10px] uppercase tracking-wider text-muted">Input</div>
            <pre className="px-3 pb-2 text-xs font-mono text-secondary whitespace-pre-wrap break-all overflow-x-auto">
              {inputStr}
            </pre>
          </div>

          {/* Result */}
          {result && (
            <div
              className={`border-t ${
                result.isError
                  ? 'border-red-500/20 bg-red-500/5'
                  : 'border-emerald-500/20 bg-emerald-500/5'
              }`}
            >
              <div className="px-3 py-1.5 text-[10px] uppercase tracking-wider text-muted">
                Result
              </div>
              <pre
                className={`px-3 pb-2 text-xs font-mono whitespace-pre-wrap break-all overflow-x-auto ${
                  result.isError ? 'text-red-400' : 'text-secondary'
                }`}
              >
                {result.content}
              </pre>
            </div>
          )}
        </>
      )}
    </div>
  );
}

// ─── Sub-agent tracker ──────────────────────────────────────────────────────

interface SubAgentTask {
  id: string;
  goal: string;
}

const TERMINAL = new Set(['success', 'failure', 'cancelled', 'timed_out']);

function SubAgentTracker({ tasks }: { tasks?: SubAgentTask[] }) {
  const [runs, setRuns] = useState<Map<string, RunSummary>>(new Map());
  const [pollingDone, setPollingDone] = useState(false);

  // Poll for sub-agent runs — they appear as tasks named "sub-agent:{id}"
  useEffect(() => {
    if (!tasks || tasks.length === 0) return;

    let cancelled = false;
    const poll = async () => {
      try {
        const active: RunSummary[] = await runsApi.getActive();
        const all: RunSummary[] = await runsApi.list({ limit: 50 });
        const combined = [...active, ...all];

        const matched = new Map<string, RunSummary>();
        for (const task of tasks) {
          const run = combined.find(
            (r) => r.isSubAgent && r.taskName === `sub-agent:${task.id}`
          );
          if (run) matched.set(task.id, run);
        }
        if (!cancelled) {
          setRuns(matched);
          const allTerminal = tasks.every((t) => {
            const r = matched.get(t.id);
            return r && TERMINAL.has(r.state);
          });
          if (allTerminal) setPollingDone(true);
        }
      } catch {
        // ignore polling errors
      }
    };

    poll();
    const interval = setInterval(poll, 2_000);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [tasks?.length]);

  // Also listen for real-time state changes
  useEffect(() => {
    const unsub = onRunStateChanged((payload) => {
      setRuns((prev) => {
        const next = new Map(prev);
        for (const [taskId, run] of next) {
          if (run.id === payload.runId) {
            next.set(taskId, { ...run, state: payload.newState as RunSummary['state'] });
          }
        }
        return next;
      });
    });
    return () => { unsub.then((fn) => fn()); };
  }, []);

  if (!tasks || tasks.length === 0) return null;
  if (pollingDone) return null;

  return (
    <div className="border-t border-warning/10 px-3 py-2 space-y-1.5">
      <div className="flex items-center gap-1.5 text-[10px] uppercase tracking-wider text-muted">
        <GitBranch size={10} />
        Sub-agents
      </div>
      {tasks.map((task) => {
        const run = runs.get(task.id);
        const state = run?.state ?? 'pending';
        return (
          <SubAgentRow key={task.id} task={task} run={run} state={state} />
        );
      })}
    </div>
  );
}

function SubAgentRow({ task, run, state }: { task: SubAgentTask; run?: RunSummary; state: string }) {
  const [expanded, setExpanded] = useState(false);
  const [toolCalls, setToolCalls] = useState<{ name: string; isError: boolean }[] | null>(null);
  const canExpand = run && TERMINAL.has(state);

  useEffect(() => {
    if (!expanded || !run) return;
    if (toolCalls !== null) return; // already loaded

    runsApi.getConversation(run.id).then((conversation) => {
      if (!conversation) { setToolCalls([]); return; }
      const calls: { name: string; isError: boolean }[] = [];
      for (const msg of conversation) {
        for (const block of msg.content) {
          if (block.type === 'tool_use') {
            // Find the matching tool_result
            const resultBlock = conversation
              .flatMap((m) => m.content)
              .find((b) => b.type === 'tool_result' && 'tool_use_id' in b && b.tool_use_id === block.id);
            calls.push({
              name: block.name,
              isError: resultBlock?.type === 'tool_result' && 'is_error' in resultBlock ? resultBlock.is_error : false,
            });
          }
        }
      }
      setToolCalls(calls);
    }).catch(() => setToolCalls([]));
  }, [expanded, run?.id]);

  return (
    <div className="rounded bg-background/50 overflow-hidden">
      <button
        onClick={() => canExpand && setExpanded((v) => !v)}
        className={`flex items-center gap-2 px-2 py-1 w-full text-left text-xs ${canExpand ? 'cursor-pointer hover:bg-background/80' : ''}`}
      >
        <SubAgentStatusIcon state={state} />
        <span className="font-medium text-white">{task.id}</span>
        <span className="text-muted truncate flex-1">{task.goal.slice(0, 60)}</span>
        {canExpand && (
          <ChevronDown
            size={10}
            className={`text-muted shrink-0 transition-transform ${expanded ? 'rotate-180' : ''}`}
          />
        )}
      </button>
      {expanded && toolCalls !== null && toolCalls.length > 0 && (
        <div className="px-2 pb-1.5 flex flex-wrap gap-1">
          {toolCalls.map((tc, i) => (
            <span
              key={i}
              className={`inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-mono ${
                tc.isError
                  ? 'bg-red-500/10 text-red-400 border border-red-500/20'
                  : 'bg-emerald-500/10 text-emerald-400 border border-emerald-500/20'
              }`}
            >
              {tc.isError ? <XCircle size={8} /> : <CheckCircle size={8} />}
              {tc.name}
            </span>
          ))}
        </div>
      )}
      {expanded && toolCalls !== null && toolCalls.length === 0 && (
        <div className="px-2 pb-1.5 text-[10px] text-muted italic">No tools used</div>
      )}
      {expanded && toolCalls === null && (
        <div className="px-2 pb-1.5 flex items-center gap-1 text-[10px] text-muted">
          <Loader2 size={8} className="animate-spin" /> Loading...
        </div>
      )}
    </div>
  );
}

function SubAgentStatusIcon({ state }: { state: string }) {
  switch (state) {
    case 'success':
      return <CheckCircle size={12} className="text-emerald-400 shrink-0" />;
    case 'failure':
      return <XCircle size={12} className="text-red-400 shrink-0" />;
    case 'cancelled':
      return <XCircle size={12} className="text-muted shrink-0" />;
    case 'timed_out':
      return <XCircle size={12} className="text-amber-400 shrink-0" />;
    case 'running':
      return <Loader2 size={12} className="text-accent-hover shrink-0 animate-spin" />;
    default:
      return <Loader2 size={12} className="text-muted shrink-0 animate-spin" />;
  }
}
