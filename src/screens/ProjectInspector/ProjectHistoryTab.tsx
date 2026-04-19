import { useQuery } from '@tanstack/react-query';
import { History } from 'lucide-react';
import { runsApi } from '../../api/runs';
import { workflowRunsApi } from '../../api/workflowRuns';
import { RunSummary, WorkflowRunSummary } from '../../types';
import { StatusBadge } from '../../components/StatusBadge';
import { useUiStore } from '../../store/uiStore';

function formatDuration(ms: number | null): string {
  if (ms === null) return '—';
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  return `${Math.floor(ms / 60000)}m ${Math.floor((ms % 60000) / 1000)}s`;
}

function timeAgo(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  if (diff < 60000) return 'just now';
  if (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`;
  if (diff < 86400000) return `${Math.floor(diff / 3600000)}h ago`;
  return `${Math.floor(diff / 86400000)}d ago`;
}

type ProjectHistoryEntry =
  | {
      id: string;
      createdAt: string;
      status: string;
      durationLabel: string;
      subtitle: string;
      title: string;
      kind: 'run';
      run: RunSummary;
    }
  | {
      id: string;
      createdAt: string;
      status: string;
      durationLabel: string;
      subtitle: string;
      title: string;
      kind: 'workflow-run';
      run: WorkflowRunSummary;
    };

function formatWorkflowDuration(run: WorkflowRunSummary): string {
  if (!run.startedAt) return '—';
  const end = run.completedAt ? new Date(run.completedAt).getTime() : Date.now();
  const start = new Date(run.startedAt).getTime();
  return formatDuration(Math.max(0, end - start));
}

export function ProjectHistoryTab({ projectId }: { projectId: string }) {
  const { navigate, openWorkflowEditor, selectRun } = useUiStore();

  const { data: runs = [], isLoading: runsLoading } = useQuery<RunSummary[]>({
    queryKey: ['runs', 'project', projectId],
    queryFn: () => runsApi.list({ projectId, limit: 50 }),
    refetchInterval: 10_000,
  });

  const { data: workflowRuns = [], isLoading: workflowRunsLoading } = useQuery<WorkflowRunSummary[]>(
    {
      queryKey: ['workflow-runs', 'project', projectId],
      queryFn: () => workflowRunsApi.listForProject(projectId, 50),
      refetchInterval: 10_000,
    }
  );

  const history: ProjectHistoryEntry[] = [
    ...runs.map((run) => ({
      id: run.id,
      createdAt: run.createdAt,
      status: run.state,
      durationLabel: formatDuration(run.durationMs),
      subtitle: `${run.agentName ?? 'unknown agent'} · ${timeAgo(run.createdAt)}`,
      title: run.taskName,
      kind: 'run' as const,
      run,
    })),
    ...workflowRuns.map((run) => ({
      id: run.id,
      createdAt: run.createdAt,
      status: run.status,
      durationLabel: formatWorkflowDuration(run),
      subtitle: `${run.triggerKind} workflow · ${timeAgo(run.createdAt)}`,
      title: run.workflowName,
      kind: 'workflow-run' as const,
      run,
    })),
  ].sort((a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime());

  if (runsLoading || workflowRunsLoading) {
    return (
      <div className="flex items-center justify-center h-32 text-muted text-sm">Loading…</div>
    );
  }

  if (history.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-2 text-muted">
        <History size={32} className="opacity-30" />
        <p className="text-sm">No runs yet</p>
        <p className="text-xs text-center max-w-xs">
          Task runs and workflow runs for this project will appear here.
        </p>
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full overflow-y-auto">
      <div className="px-4 py-3 border-b border-edge">
        <h3 className="text-sm font-semibold text-white">
          Run History
          <span className="ml-2 text-xs text-muted font-normal">({history.length})</span>
        </h3>
      </div>

      <ul className="divide-y divide-edge">
        {history.map((entry) => (
          <li
            key={`${entry.kind}-${entry.id}`}
            onClick={() => {
              if (entry.kind === 'run') {
                selectRun(entry.run.id);
                navigate('history');
                return;
              }
              openWorkflowEditor(entry.run.workflowId);
            }}
            className="flex items-center gap-3 px-4 py-3 hover:bg-surface cursor-pointer transition-colors"
          >
            <StatusBadge state={entry.status} />
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2 min-w-0">
                <p className="text-sm font-medium text-white truncate">{entry.title}</p>
                <span className="text-[10px] uppercase tracking-wider text-muted shrink-0">
                  {entry.kind === 'run' ? 'task' : 'workflow'}
                </span>
              </div>
              <p className="text-xs text-muted">{entry.subtitle}</p>
            </div>
            <span className="text-xs text-muted shrink-0">{entry.durationLabel}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
