import { useState, type ReactNode } from 'react';
import { ChevronRight } from 'lucide-react';
import { cn } from '../lib/cn';

interface CollapsibleSectionProps {
  title: string;
  description?: string;
  defaultOpen?: boolean;
  badge?: ReactNode;
  children: ReactNode;
  className?: string;
}

export function CollapsibleSection({
  title,
  description,
  defaultOpen = false,
  badge,
  children,
  className,
}: CollapsibleSectionProps) {
  const [open, setOpen] = useState(defaultOpen);

  return (
    <div className={cn('rounded-xl border border-edge bg-surface overflow-hidden', className)}>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-2 w-full px-4 py-3 text-left hover:bg-surface-hover transition-colors"
      >
        <ChevronRight
          size={14}
          className={cn('text-muted shrink-0 transition-transform', open && 'rotate-90')}
        />
        <div className="flex-1 min-w-0">
          <span className="text-sm font-semibold text-white">{title}</span>
          {description && (
            <p className="text-xs text-muted mt-0.5 leading-tight">{description}</p>
          )}
        </div>
        {badge && <div className="shrink-0">{badge}</div>}
      </button>
      {open && (
        <div className="border-t border-edge px-4 py-4">
          {children}
        </div>
      )}
    </div>
  );
}
