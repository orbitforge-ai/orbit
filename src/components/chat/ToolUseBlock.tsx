import { useEffect, useState } from 'react';
import {
  AlertTriangle,
  CheckCircle,
  XCircle,
  Loader2,
  GitBranch,
  PauseCircle,
  HelpCircle,
  ChevronDown,
} from 'lucide-react';
import { onRunStateChanged } from '../../events/runEvents';
import { runsApi } from '../../api/runs';
import { RunSummary } from '../../types';
import { DisplayBlock } from './types';
import { formatToolName, getToolVisual } from './toolVisuals';
import { ThinkingBlock, ThinkingDetailPanel } from './ThinkingBlock';
import {
  buildToolPresentation,
  canExpandToolDetails,
  ToolPresentationSections,
} from './toolPresentation';
import { useSettingsStore } from '../../store/settingsStore';

interface ToolUseBlockProps {
  thoughts: Extract<DisplayBlock, { kind: 'thinking' }>[];
  tools: Extract<DisplayBlock, { kind: 'tool_call' }>[];
  expandedItemKey: string | null;
  onExpandedItemChange: (itemKey: string | null) => void;
  allowDetails: boolean;
}

const INTERRUPTED_TOOL_RESULT_MARKER =
  'did not complete because the previous response was interrupted';

export function ToolUseBlock({
  thoughts,
  tools,
  expandedItemKey,
  onExpandedItemChange,
  allowDetails,
}: ToolUseBlockProps) {
  const expandedThinking =
    allowDetails && expandedItemKey?.startsWith('thinking:')
      ? thoughts[Number(expandedItemKey.split(':')[1])] ?? null
      : null;
  const expandedTool =
    allowDetails && expandedItemKey?.startsWith('tool:')
      ? tools.find(
          (tool) => `tool:${tool.id}` === expandedItemKey && canExpandToolDetails(tool)
        ) ?? null
      : null;

  if (tools.length === 0 && thoughts.length === 0) return null;

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap gap-1.5">
        {thoughts.map((thought, index) => {
          const itemKey = `thinking:${index}`;
          return (
            <ThinkingBlock
              key={itemKey}
              thinking={thought.thinking}
              selected={allowDetails && expandedItemKey === itemKey}
              disabled={!allowDetails}
              onClick={() => {
                if (!allowDetails) return;
                onExpandedItemChange(expandedItemKey === itemKey ? null : itemKey);
              }}
            />
          );
        })}
        {tools.map((tool) => (
          <ToolChip
            key={tool.id}
            tool={tool}
            selected={
              allowDetails &&
              canExpandToolDetails(tool) &&
              expandedItemKey === `tool:${tool.id}`
            }
            disabled={!allowDetails || !canExpandToolDetails(tool)}
            onClick={() => {
              if (!allowDetails || !canExpandToolDetails(tool)) return;
              onExpandedItemChange(
                expandedItemKey === `tool:${tool.id}` ? null : `tool:${tool.id}`
              );
            }}
          />
        ))}
      </div>

      {expandedThinking && allowDetails && <ThinkingDetailPanel thinking={expandedThinking.thinking} />}
      {expandedTool && allowDetails && <ToolDetailPanel tool={expandedTool} />}
    </div>
  );
}

function ToolChip({
  tool,
  selected,
  disabled,
  onClick,
}: {
  tool: Extract<DisplayBlock, { kind: 'tool_call' }>;
  selected: boolean;
  disabled: boolean;
  onClick: () => void;
}) {
  const { Icon, colorClass } = getToolVisual(tool.name);
  const label = formatToolName(tool.name);
  const status = getToolStatus(tool);
  const interactiveClasses = selected
    ? 'border-warning/50 bg-warning/15 text-warning shadow-[0_0_0_1px_rgba(245,158,11,0.12)]'
    : 'border-edge bg-background text-secondary hover:border-edge-hover hover:text-white';

  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={`inline-flex max-w-full items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] transition-colors ${interactiveClasses} ${
        disabled ? 'cursor-default opacity-90' : ''
      }`}
      title={label}
    >
      <Icon size={11} className={`${colorClass} shrink-0`} />
      <span className="truncate font-medium">{label}</span>
      <ToolStatusIndicator status={status} />
    </button>
  );
}

function ToolDetailPanel({ tool }: { tool: Extract<DisplayBlock, { kind: 'tool_call' }> }) {
  const showVerboseToolDetails = useSettingsStore((s) => s.showVerboseToolDetails);
  const label = formatToolName(tool.name);
  const isSubAgentTool = tool.name === 'spawn_sub_agents';
  const isWaitingSendMessage =
    tool.name === 'send_message' && tool.input.wait_for_result === true && !tool.result;
  const isWaitingYield = tool.name === 'yield_turn' && !tool.result;
  const isWaitingAskUser = tool.name === 'ask_user' && !tool.result;
  const status = getToolStatus(tool);
  const isInterrupted = status === 'interrupted';
  const presentation = buildToolPresentation(tool);
  const inputStr = tool.inputText ?? JSON.stringify(tool.input, null, 2);
  const resultStr = tool.result?.content ?? null;

  return (
    <div className="rounded-lg border border-warning/25 bg-warning/5 overflow-hidden">
      <div className="flex items-center gap-2 px-3 py-2 border-b border-warning/10">
        <div className="inline-flex items-center gap-1.5 rounded-full border border-warning/20 bg-background/80 px-2 py-1 text-[11px]">
          {(() => {
            const { Icon, colorClass } = getToolVisual(tool.name);
            return <Icon size={11} className={`${colorClass} shrink-0`} />;
          })()}
          <span className="font-medium text-warning">{label}</span>
          <ToolStatusIndicator status={status} />
        </div>
      </div>

      {isSubAgentTool && !tool.result && (
        <SubAgentTracker tasks={tool.input.tasks as SubAgentTask[] | undefined} />
      )}
      {isWaitingSendMessage && (
        <SendMessagePending
          targetAgent={
            typeof tool.input.target_agent === 'string' ? tool.input.target_agent : undefined
          }
        />
      )}
      {isWaitingYield && (
        <GenericPending
          Icon={PauseCircle}
          label="Waiting for the selected yield condition"
          sublabel={
            typeof tool.input.wait_for === 'string'
              ? `Condition: ${tool.input.wait_for}`
              : undefined
          }
        />
      )}
      {isWaitingAskUser && (
        <GenericPending
          Icon={HelpCircle}
          label="Waiting for the user to respond"
          sublabel={
            typeof tool.input.question === 'string' ? tool.input.question : undefined
          }
        />
      )}

      {presentation.requestSections.length > 0 && (
        <div className="border-t border-warning/10">
          <ToolPresentationSections sections={presentation.requestSections} />
        </div>
      )}

      {tool.result && (
        <div
          className={`border-t ${
            isInterrupted
              ? 'border-amber-500/20 bg-amber-500/5'
              : tool.result.isError
              ? 'border-red-500/20 bg-red-500/5'
              : 'border-emerald-500/20 bg-emerald-500/5'
          }`}
        >
          {isInterrupted && (
            <div className="px-3 pt-3 text-xs text-amber-300">
              This tool call was interrupted before it completed. The agent should retry this step
              in smaller pieces.
            </div>
          )}
          <ToolPresentationSections sections={presentation.resultSections} />
        </div>
      )}

      {showVerboseToolDetails && (
        <div className="border-t border-warning/10">
          <div className="space-y-3 px-3 py-3">
            <section className="space-y-1.5">
              <div className="text-[10px] uppercase tracking-wider text-muted">Raw Input</div>
              <pre className="rounded-lg border border-edge bg-background/60 px-3 py-2 text-xs font-mono text-secondary whitespace-pre-wrap break-all overflow-x-auto">
                {inputStr}
              </pre>
            </section>
            {resultStr && (
              <section className="space-y-1.5">
                <div className="text-[10px] uppercase tracking-wider text-muted">Raw Result</div>
                <pre
                  className={`rounded-lg border px-3 py-2 text-xs font-mono whitespace-pre-wrap break-all overflow-x-auto ${
                    isInterrupted
                      ? 'border-amber-500/20 bg-amber-500/5 text-amber-200'
                      : tool.result?.isError
                        ? 'border-red-500/20 bg-red-500/5 text-red-200'
                        : 'border-edge bg-background/60 text-secondary'
                  }`}
                >
                  {resultStr}
                </pre>
              </section>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function getToolStatus(tool: Extract<DisplayBlock, { kind: 'tool_call' }>) {
  const isSubAgentTool = tool.name === 'spawn_sub_agents';
  const isWaitingSendMessage =
    tool.name === 'send_message' && tool.input.wait_for_result === true && !tool.result;
  const isWaitingYield = tool.name === 'yield_turn' && !tool.result;
  const isWaitingAskUser = tool.name === 'ask_user' && !tool.result;
  const isInterrupted =
    tool.result?.isError && tool.result.content.includes(INTERRUPTED_TOOL_RESULT_MARKER);

  if (isInterrupted) return 'interrupted' as const;
  if (tool.result?.isError) return 'error' as const;
  if (tool.result) return 'success' as const;
  if (isSubAgentTool || isWaitingSendMessage || isWaitingYield || isWaitingAskUser) {
    return 'pending' as const;
  }
  return 'idle' as const;
}

function ToolStatusIndicator({
  status,
}: {
  status: 'success' | 'error' | 'pending' | 'idle' | 'interrupted';
}) {
  switch (status) {
    case 'success':
      return <CheckCircle size={11} className="text-emerald-400 shrink-0" />;
    case 'interrupted':
      return <AlertTriangle size={11} className="text-amber-400 shrink-0" />;
    case 'error':
      return <XCircle size={11} className="text-red-400 shrink-0" />;
    case 'pending':
      return <Loader2 size={11} className="text-accent-hover shrink-0 animate-spin" />;
    default:
      return <span className="h-1.5 w-1.5 rounded-full bg-border-hover shrink-0" />;
  }
}

function SendMessagePending({ targetAgent }: { targetAgent?: string }) {
  const [elapsedSeconds, setElapsedSeconds] = useState(0);

  useEffect(() => {
    const interval = setInterval(() => {
      setElapsedSeconds((seconds) => seconds + 1);
    }, 1000);

    return () => clearInterval(interval);
  }, []);

  return (
    <div className="border-t border-warning/10 px-3 py-2">
      <div className="flex items-center gap-2 text-xs text-secondary">
        <Loader2 size={12} className="text-accent-hover animate-spin shrink-0" />
        <span>
          Waiting for {targetAgent ? `"${targetAgent}"` : 'target agent'} to finish and return a
          result
        </span>
      </div>
      <div className="mt-1 pl-5 text-[11px] text-muted">
        {elapsedSeconds < 60
          ? `${elapsedSeconds}s elapsed`
          : `${Math.floor(elapsedSeconds / 60)}m ${elapsedSeconds % 60}s elapsed`}
      </div>
    </div>
  );
}

function GenericPending({
  Icon,
  label,
  sublabel,
}: {
  Icon: typeof PauseCircle;
  label: string;
  sublabel?: string;
}) {
  return (
    <div className="border-t border-warning/10 px-3 py-2">
      <div className="flex items-center gap-2 text-xs text-secondary">
        <Icon size={12} className="text-accent-hover shrink-0" />
        <span>{label}</span>
      </div>
      {sublabel && <div className="mt-1 pl-5 text-[11px] text-muted">{sublabel}</div>}
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
          const run = combined.find((r) => r.isSubAgent && r.taskName === `sub-agent:${task.id}`);
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
    return () => {
      unsub.then((fn) => fn()).catch(() => {});
    };
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
        return <SubAgentRow key={task.id} task={task} run={run} state={state} />;
      })}
    </div>
  );
}

function SubAgentRow({
  task,
  run,
  state,
}: {
  task: SubAgentTask;
  run?: RunSummary;
  state: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const [toolCalls, setToolCalls] = useState<{ name: string; isError: boolean }[] | null>(null);
  const canExpand = run && TERMINAL.has(state);

  useEffect(() => {
    if (!expanded || !run) return;
    if (toolCalls !== null) return; // already loaded

    runsApi
      .getConversation(run.id)
      .then((conversation) => {
        if (!conversation) {
          setToolCalls([]);
          return;
        }
        const calls: { name: string; isError: boolean }[] = [];
        for (const msg of conversation) {
          for (const block of msg.content) {
            if (block.type === 'tool_use') {
              // Find the matching tool_result
              const resultBlock = conversation
                .flatMap((m) => m.content)
                .find(
                  (b) =>
                    b.type === 'tool_result' && 'tool_use_id' in b && b.tool_use_id === block.id
                );
              calls.push({
                name: block.name,
                isError:
                  resultBlock?.type === 'tool_result' && 'is_error' in resultBlock
                    ? resultBlock.is_error
                    : false,
              });
            }
          }
        }
        setToolCalls(calls);
      })
      .catch(() => setToolCalls([]));
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
              {formatToolName(tc.name)}
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
