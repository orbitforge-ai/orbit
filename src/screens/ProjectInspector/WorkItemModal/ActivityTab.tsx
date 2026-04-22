import { useQuery } from '@tanstack/react-query';
import { workItemsApi } from '../../../api/workItems';
import type { Agent, WorkItemEvent } from '../../../types';
import { renderEvent } from './activityEventRenderers';

function formatRelative(iso: string): string {
  const then = new Date(iso).getTime();
  const now = Date.now();
  const sec = Math.round((now - then) / 1000);
  if (sec < 60) return 'just now';
  if (sec < 3600) return `${Math.floor(sec / 60)}m ago`;
  if (sec < 86400) return `${Math.floor(sec / 3600)}h ago`;
  if (sec < 86400 * 7) return `${Math.floor(sec / 86400)}d ago`;
  return new Date(iso).toLocaleDateString();
}

interface Props {
  workItemId: string;
  agents: Agent[];
}

export function ActivityTab({ workItemId, agents }: Props) {
  const agentById = new Map(agents.map((a) => [a.id, a]));
  const { data: events = [], isLoading } = useQuery<WorkItemEvent[]>({
    queryKey: ['work-items', workItemId, 'events'],
    queryFn: () => workItemsApi.listEvents(workItemId),
  });

  if (isLoading) return <div className="text-xs text-muted">Loading…</div>;
  if (events.length === 0) {
    return (
      <div className="rounded-lg border border-dashed border-edge px-3 py-4 text-center text-xs text-muted italic">
        No activity yet.
      </div>
    );
  }

  const ordered = [...events].reverse();

  return (
    <ol className="relative space-y-3 border-l border-edge pl-4 pt-1">
      {ordered.map((ev) => {
        const { Icon, iconClass, title, subline } = renderEvent(ev);
        const actor =
          ev.actorKind === 'agent' && ev.actorAgentId
            ? agentById.get(ev.actorAgentId)?.name ?? 'Agent'
            : ev.actorKind === 'user'
              ? 'You'
              : 'System';
        return (
          <li key={ev.id} className="relative">
            <span className="absolute -left-[22px] top-0.5 flex h-4 w-4 items-center justify-center rounded-full border border-edge bg-panel">
              <Icon size={10} className={iconClass} />
            </span>
            <div className="text-xs text-secondary">
              <span className="font-medium text-white">{actor}</span> {title}
              <span className="ml-1.5 text-[10px] text-muted">· {formatRelative(ev.createdAt)}</span>
            </div>
            {subline && <div className="mt-0.5 text-[11px] text-secondary">{subline}</div>}
          </li>
        );
      })}
    </ol>
  );
}
