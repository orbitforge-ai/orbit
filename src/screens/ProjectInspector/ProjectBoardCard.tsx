import { Bot, Bug, FileText, Lightbulb, ListChecks, Wrench } from 'lucide-react';
import { Agent, WorkItem } from '../../types';
import { cn } from '../../lib/cn';
import { formatWorkItemId } from '../../lib/workItemId';

const KIND_ICON_MAP: Record<string, typeof ListChecks> = {
  task: ListChecks,
  bug: Bug,
  story: FileText,
  spike: Lightbulb,
  chore: Wrench,
};

const KIND_COLOR_MAP: Record<string, string> = {
  task: 'text-accent-hover',
  bug: 'text-red-400',
  story: 'text-blue-400',
  spike: 'text-amber-400',
  chore: 'text-muted',
};

const PRIORITY_DOT: Record<number, string> = {
  0: 'bg-muted/40',
  1: 'bg-blue-400',
  2: 'bg-amber-400',
  3: 'bg-red-400',
};

const PRIORITY_LABEL: Record<number, string> = {
  0: 'Low priority',
  1: 'Medium priority',
  2: 'High priority',
  3: 'Urgent',
};

export function ProjectBoardCard({
  item,
  boardPrefix,
  assignee,
  onClick,
}: {
  item: WorkItem;
  boardPrefix: string | null;
  assignee: Agent | null;
  onClick: () => void;
}) {
  const KindIcon = KIND_ICON_MAP[item.kind] ?? ListChecks;
  const kindColor = KIND_COLOR_MAP[item.kind] ?? 'text-muted';
  const displayId = formatWorkItemId(boardPrefix, item.id);

  return (
    <div
      onClick={onClick}
      className={cn(
        'group rounded-lg border border-edge bg-surface px-3 py-2 cursor-pointer',
        'hover:border-accent/40 hover:bg-surface/80 transition-colors',
      )}
    >
      <div className="flex items-start gap-2">
        <KindIcon size={12} className={cn('mt-0.5 shrink-0', kindColor)} />
        <div className="flex-1 min-w-0">
          <div className="mb-1 flex items-center justify-between gap-2">
            <span className="rounded bg-edge px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-wider text-muted">
              {displayId}
            </span>
          </div>
          <p className="text-xs font-medium text-white line-clamp-2">{item.title}</p>
          {item.status === 'blocked' && item.blockedReason && (
            <p className="mt-1 text-[10px] text-red-300 line-clamp-2 italic">
              ⛔ {item.blockedReason}
            </p>
          )}
          {item.labels.length > 0 && (
            <div className="mt-1.5 flex flex-wrap gap-1">
              {item.labels.slice(0, 4).map((label) => (
                <span
                  key={label}
                  className="rounded bg-edge px-1.5 py-0.5 text-[9px] font-medium text-muted"
                >
                  {label}
                </span>
              ))}
              {item.labels.length > 4 && (
                <span className="text-[9px] text-muted">+{item.labels.length - 4}</span>
              )}
            </div>
          )}
        </div>
      </div>
      <div className="mt-2 flex items-center justify-between">
        <span
          className={cn('inline-block w-1.5 h-1.5 rounded-full', PRIORITY_DOT[item.priority] ?? PRIORITY_DOT[0])}
          title={PRIORITY_LABEL[item.priority] ?? PRIORITY_LABEL[0]}
        />
        {assignee ? (
          <span
            className="flex items-center gap-1 text-[10px] text-muted"
            title={`Assigned to ${assignee.name}`}
          >
            <Bot size={10} />
            <span className="truncate max-w-[100px]">{assignee.name}</span>
          </span>
        ) : (
          <span className="text-[10px] text-muted/60">Unassigned</span>
        )}
      </div>
    </div>
  );
}
