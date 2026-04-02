import { useQuery } from '@tanstack/react-query';
import { History } from 'lucide-react';
import { runsApi } from '../../api/runs';
import { RunSummary } from '../../types';
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

export function ProjectHistoryTab({ projectId }: { projectId: string }) {
  const { selectRun, navigate } = useUiStore();

  const { data: runs = [], isLoading } = useQuery<RunSummary[]>({
    queryKey: ['runs', 'project', projectId],
    queryFn: () => runsApi.list({ projectId, limit: 50 }),
    refetchInterval: 10_000,
  });

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-32 text-muted text-sm">Loading…</div>
    );
  }

  if (runs.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-2 text-muted">
        <History size={32} className="opacity-30" />
        <p className="text-sm">No runs yet</p>
        <p className="text-xs text-center max-w-xs">
          Runs from tasks assigned to this project will appear here.
        </p>
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full overflow-y-auto">
      <div className="px-4 py-3 border-b border-edge">
        <h3 className="text-sm font-semibold text-white">
          Run History
          <span className="ml-2 text-xs text-muted font-normal">({runs.length})</span>
        </h3>
      </div>

      <ul className="divide-y divide-edge">
        {runs.map((run) => (
          <li
            key={run.id}
            onClick={() => {
              selectRun(run.id);
              navigate('history');
            }}
            className="flex items-center gap-3 px-4 py-3 hover:bg-surface cursor-pointer transition-colors"
          >
            <StatusBadge state={run.state} />
            <div className="flex-1 min-w-0">
              <p className="text-sm font-medium text-white truncate">{run.taskName}</p>
              <p className="text-xs text-muted">
                {run.agentName ?? 'unknown agent'} · {timeAgo(run.createdAt)}
              </p>
            </div>
            <span className="text-xs text-muted shrink-0">
              {formatDuration(run.durationMs)}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}
