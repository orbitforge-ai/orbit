import { AlertOctagon, CheckCircle, Circle, CircleDashed, Eye, Loader2, Slash } from 'lucide-react';
import type { WorkItemStatus } from '../../../types';
import { cn } from '../../../lib/cn';

const CONFIG: Record<
  WorkItemStatus,
  { label: string; tone: string; Icon: React.ComponentType<{ size?: number; className?: string }> }
> = {
  backlog: { label: 'Backlog', tone: 'text-muted', Icon: CircleDashed },
  todo: { label: 'Todo', tone: 'text-secondary', Icon: Circle },
  in_progress: { label: 'In Progress', tone: 'text-accent-light', Icon: Loader2 },
  review: { label: 'In Review', tone: 'text-yellow-400', Icon: Eye },
  blocked: { label: 'Blocked', tone: 'text-red-400', Icon: AlertOctagon },
  done: { label: 'Done', tone: 'text-emerald-400', Icon: CheckCircle },
  cancelled: { label: 'Cancelled', tone: 'text-muted', Icon: Slash },
};

interface Props {
  status: WorkItemStatus;
  className?: string;
}

export function StatusBadge({ status, className }: Props) {
  const cfg = CONFIG[status] ?? CONFIG.todo;
  const Icon = cfg.Icon;
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1.5 rounded-full border border-edge bg-surface px-2.5 py-0.5 text-xs font-medium',
        cfg.tone,
        className,
      )}
    >
      <Icon size={11} />
      {cfg.label}
    </span>
  );
}
