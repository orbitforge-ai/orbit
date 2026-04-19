import { useEffect, useMemo, useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  AlertCircle,
  CheckCircle,
  ChevronRight,
  Clock,
  Loader2,
  Play,
  StopCircle,
  X,
} from 'lucide-react';
import { workflowRunsApi } from '../../api/workflowRuns';
import {
  WorkflowRun,
  WorkflowRunStatus,
  WorkflowRunStepStatus,
  WorkflowRunStep,
  WorkflowRunWithSteps,
} from '../../types';

function formatDuration(
  startedAt: string | null,
  completedAt: string | null,
  nowMs: number,
): string {
  if (!startedAt) return '—';
  const end = completedAt ? new Date(completedAt).getTime() : nowMs;
  const start = new Date(startedAt).getTime();
  const ms = Math.max(0, end - start);
  if (ms < 1000) return `${ms}ms`;
  const s = Math.round(ms / 100) / 10;
  if (s < 60) return `${s.toFixed(1)}s`;
  const m = Math.floor(s / 60);
  const rem = Math.round(s - m * 60);
  return `${m}m ${rem}s`;
}

function formatRelative(iso: string, nowMs: number): string {
  const diff = nowMs - new Date(iso).getTime();
  const s = Math.floor(diff / 1000);
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  return new Date(iso).toLocaleString();
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function asString(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value : null;
}

function resolveStepNodeSubtitle(step: WorkflowRunStep, detail: WorkflowRunWithSteps): string {
  if (step.nodeType !== 'board.work_item.create') {
    return step.nodeType;
  }

  const snapshotNode = detail.graphSnapshot.nodes.find((node) => node.id === step.nodeId);
  const inputAction = isRecord(step.input) ? asString(step.input.action) : null;
  const outputAction = isRecord(step.output) ? asString(step.output.action) : null;
  const snapshotAction = snapshotNode ? asString(snapshotNode.data.action) : null;
  const action = inputAction ?? outputAction ?? snapshotAction;

  return action ? `board.work_item.${action}` : step.nodeType;
}

function StatusIcon({
  status,
  size = 14,
}: {
  status: WorkflowRunStatus | WorkflowRunStepStatus;
  size?: number;
}) {
  switch (status) {
    case 'running':
    case 'queued':
      return <Loader2 size={size} className="text-blue-300 animate-spin" />;
    case 'success':
      return <CheckCircle size={size} className="text-emerald-400" />;
    case 'failed':
      return <AlertCircle size={size} className="text-red-400" />;
    case 'cancelled':
    case 'skipped':
      return <StopCircle size={size} className="text-muted" />;
    default:
      return <Clock size={size} className="text-muted" />;
  }
}

export function RunHistoryDrawer({
  workflowId,
  focusRunId,
  onClose,
}: {
  workflowId: string;
  focusRunId?: string | null;
  onClose: () => void;
}) {
  const queryClient = useQueryClient();
  const [selectedRunId, setSelectedRunId] = useState<string | null>(focusRunId ?? null);
  const [nowMs, setNowMs] = useState(() => Date.now());

  useEffect(() => {
    const interval = window.setInterval(() => {
      setNowMs(Date.now());
    }, 1000);
    return () => window.clearInterval(interval);
  }, []);

  const { data: runs = [], isLoading } = useQuery<WorkflowRun[]>({
    queryKey: ['workflow-runs', workflowId],
    queryFn: () => workflowRunsApi.list(workflowId),
  });

  useEffect(() => {
    if (focusRunId) {
      setSelectedRunId(focusRunId);
    }
  }, [focusRunId]);

  useEffect(() => {
    if (!selectedRunId && runs.length > 0) {
      setSelectedRunId(runs[0].id);
    }
  }, [runs, selectedRunId]);

  const { data: detail } = useQuery<WorkflowRunWithSteps>({
    queryKey: ['workflow-run', selectedRunId],
    queryFn: () => workflowRunsApi.get(selectedRunId!),
    enabled: !!selectedRunId,
  });

  const cancelMutation = useMutation({
    mutationFn: (runId: string) => workflowRunsApi.cancel(runId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['workflow-runs', workflowId] });
      if (selectedRunId) {
        queryClient.invalidateQueries({ queryKey: ['workflow-run', selectedRunId] });
      }
    },
  });

  return (
    <div
      className="fixed inset-0 z-50 flex justify-end bg-black/50"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-[880px] h-full bg-panel border-l border-edge shadow-2xl flex flex-col">
        <div className="flex items-center justify-between px-5 py-3 border-b border-edge shrink-0">
          <div className="flex items-center gap-2">
            <span className="text-[10px] uppercase tracking-wide text-muted">Run history</span>
            <span className="text-[10px] text-muted font-mono">
              {runs.length} run{runs.length === 1 ? '' : 's'}
            </span>
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded text-muted hover:text-white hover:bg-edge transition-colors"
            aria-label="Close"
          >
            <X size={16} />
          </button>
        </div>

        <div className="flex-1 min-h-0 flex">
          <div className="w-[300px] border-r border-edge overflow-y-auto shrink-0">
            {isLoading ? (
              <div className="p-4 text-xs text-muted">Loading…</div>
            ) : runs.length === 0 ? (
              <div className="p-4 text-xs text-muted">
                No runs yet. Click{' '}
                <span className="inline-flex items-center gap-1 text-white">
                  <Play size={10} /> Run
                </span>{' '}
                to start one.
              </div>
            ) : (
              <ul>
                {runs.map((r) => {
                  const active = r.id === selectedRunId;
                  return (
                    <li key={r.id}>
                      <button
                        onClick={() => setSelectedRunId(r.id)}
                        className={`w-full text-left px-3 py-2.5 border-b border-edge/50 flex items-start gap-2 text-xs transition-colors ${
                          active ? 'bg-edge/30' : 'hover:bg-edge/20'
                        }`}
                      >
                        <div className="pt-0.5">
                          <StatusIcon status={r.status} />
                        </div>
                        <div className="flex-1 min-w-0">
                          <div className="flex items-center justify-between gap-2">
                            <span className="text-white font-mono text-[11px]">
                              {r.id.slice(-8)}
                            </span>
                            <span className="text-muted text-[10px]">v{r.workflowVersion}</span>
                          </div>
                          <div className="flex items-center gap-2 mt-0.5 text-muted text-[10px]">
                            <span>{r.triggerKind}</span>
                            <span>·</span>
                            <span>{formatRelative(r.createdAt, nowMs)}</span>
                          </div>
                          <div className="text-muted text-[10px] mt-0.5">
                            {formatDuration(r.startedAt, r.completedAt, nowMs)}
                          </div>
                        </div>
                        <ChevronRight size={12} className="text-muted shrink-0 mt-1" />
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>

          <div className="flex-1 min-w-0 overflow-y-auto">
            {!detail ? (
              <div className="p-6 text-sm text-muted">
                {selectedRunId ? 'Loading run…' : 'Select a run to see details.'}
              </div>
            ) : (
              <RunDetail
                detail={detail}
                nowMs={nowMs}
                onCancel={() => cancelMutation.mutate(detail.id)}
                cancelling={cancelMutation.isPending}
              />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function RunDetail({
  detail,
  nowMs,
  onCancel,
  cancelling,
}: {
  detail: WorkflowRunWithSteps;
  nowMs: number;
  onCancel: () => void;
  cancelling: boolean;
}) {
  const isActive = detail.status === 'queued' || detail.status === 'running';

  return (
    <div className="px-5 py-4 space-y-4">
      <div className="flex items-center gap-2">
        <StatusIcon status={detail.status} size={18} />
        <span className="text-base font-semibold text-white capitalize">{detail.status}</span>
        <span className="text-[10px] text-muted font-mono">{detail.id.slice(-12)}</span>
        <div className="flex-1" />
        {isActive && (
          <button
            onClick={onCancel}
            disabled={cancelling}
            className="flex items-center gap-1 px-2.5 py-1 rounded-md bg-red-500/10 hover:bg-red-500/20 text-red-300 text-xs font-medium transition-colors disabled:opacity-50"
          >
            <StopCircle size={12} />
            {cancelling ? 'Cancelling…' : 'Cancel'}
          </button>
        )}
      </div>

      <div className="grid grid-cols-2 gap-3 text-xs">
        <MetaRow label="Trigger" value={detail.triggerKind} />
        <MetaRow label="Version" value={`v${detail.workflowVersion}`} />
        <MetaRow
          label="Started"
          value={detail.startedAt ? new Date(detail.startedAt).toLocaleString() : '—'}
        />
        <MetaRow
          label="Duration"
          value={formatDuration(detail.startedAt, detail.completedAt, nowMs)}
        />
      </div>

      {detail.error && (
        <div className="px-3 py-2 rounded-md bg-red-500/10 border border-red-500/20 text-xs text-red-300">
          {detail.error}
        </div>
      )}

      <div>
        <h3 className="text-[10px] uppercase tracking-wide text-muted mb-2">
          Steps ({detail.steps.length})
        </h3>
        {detail.steps.length === 0 ? (
          <div className="text-xs text-muted px-3 py-4 border border-edge rounded-md">
            No steps recorded yet.
          </div>
        ) : (
          <ul className="space-y-2">
            {detail.steps.map((step) => (
              <li
                key={step.id}
                className="border border-edge rounded-md bg-surface/50 overflow-hidden"
              >
                <div className="flex items-center gap-2 px-3 py-2 border-b border-edge/50">
                  <StatusIcon status={step.status} />
                  <span className="text-xs font-mono text-white">{step.nodeId}</span>
                  <span className="text-[10px] text-muted">
                    {resolveStepNodeSubtitle(step, detail)}
                  </span>
                  <div className="flex-1" />
                  <span className="text-[10px] text-muted">
                    {formatDuration(step.startedAt, step.completedAt, nowMs)}
                  </span>
                </div>
                {step.error && (
                  <div className="px-3 py-2 bg-red-500/10 text-[11px] text-red-300 border-b border-edge/50">
                    {step.error}
                  </div>
                )}
                <StepPayload label="Input" value={step.input} />
                {step.output !== null && step.output !== undefined && (
                  <StepPayload label="Output" value={step.output} />
                )}
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

function MetaRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col">
      <span className="text-[10px] uppercase tracking-wide text-muted">{label}</span>
      <span className="text-white">{value}</span>
    </div>
  );
}

function StepPayload({ label, value }: { label: string; value: unknown }) {
  const [open, setOpen] = useState(false);
  const text = useMemo(() => {
    try {
      return JSON.stringify(value, null, 2);
    } catch {
      return String(value);
    }
  }, [value]);
  return (
    <div className="border-t border-edge/50 first:border-t-0">
      <button
        onClick={() => setOpen((p) => !p)}
        className="w-full flex items-center gap-1 px-3 py-1.5 text-[10px] uppercase tracking-wide text-muted hover:bg-edge/20"
      >
        <ChevronRight size={10} className={`transition-transform ${open ? 'rotate-90' : ''}`} />
        {label}
      </button>
      {open && (
        <pre className="px-3 py-2 text-[11px] text-muted font-mono whitespace-pre-wrap break-all bg-black/20 max-h-60 overflow-auto">
          {text}
        </pre>
      )}
    </div>
  );
}
