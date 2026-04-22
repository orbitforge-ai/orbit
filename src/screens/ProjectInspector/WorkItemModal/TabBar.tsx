import { cn } from '../../../lib/cn';

export type WorkItemModalTab = 'comments' | 'activity';

interface Props {
  value: WorkItemModalTab;
  onChange: (next: WorkItemModalTab) => void;
  commentCount?: number;
}

const TABS: { id: WorkItemModalTab; label: string }[] = [
  { id: 'comments', label: 'Comments' },
  { id: 'activity', label: 'Activity' },
];

export function TabBar({ value, onChange, commentCount }: Props) {
  return (
    <div className="flex items-center gap-1 border-b border-edge">
      {TABS.map((t) => {
        const active = t.id === value;
        const showCount = t.id === 'comments' && typeof commentCount === 'number';
        return (
          <button
            key={t.id}
            type="button"
            onClick={() => onChange(t.id)}
            className={cn(
              'relative px-3 py-2 text-xs font-medium transition-colors',
              active ? 'text-white' : 'text-muted hover:text-white',
            )}
          >
            {t.label}
            {showCount && commentCount! > 0 && (
              <span
                className={cn(
                  'ml-1.5 inline-flex min-w-[18px] justify-center rounded-full px-1.5 py-0.5 text-[10px] font-semibold',
                  active ? 'bg-accent/20 text-accent-light' : 'bg-edge text-muted',
                )}
              >
                {commentCount}
              </span>
            )}
            {active && (
              <span className="absolute inset-x-0 -bottom-px h-0.5 bg-accent" />
            )}
          </button>
        );
      })}
    </div>
  );
}
