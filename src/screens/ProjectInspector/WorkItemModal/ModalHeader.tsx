import { useEffect, useRef, useState } from 'react';
import { CheckCircle, Hash } from 'lucide-react';
import { ModalCloseButton } from '../../../components/ui/Modal';
import { formatWorkItemId } from '../../../lib/workItemId';
import { cn } from '../../../lib/cn';
import type { WorkItem } from '../../../types';
import { StatusBadge } from './StatusBadge';

interface Props {
  item: WorkItem;
  boardPrefix: string | null;
  title: string;
  onTitleChange: (next: string) => void;
  onTitleCommit: () => void;
  onComplete: () => void;
  onClose: () => void;
}

export function ModalHeader({
  item,
  boardPrefix,
  title,
  onTitleChange,
  onTitleCommit,
  onComplete,
  onClose,
}: Props) {
  const [editing, setEditing] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (editing) inputRef.current?.focus();
  }, [editing]);

  const humanId = formatWorkItemId(boardPrefix, item.id);

  return (
    <div className="flex items-center gap-3 border-b border-edge px-5 py-3">
      <span
        className="inline-flex items-center gap-1 rounded-md border border-edge bg-surface px-2 py-0.5 text-[11px] font-mono uppercase tracking-wider text-secondary"
        title="Work item identifier"
      >
        <Hash size={10} className="text-muted" />
        {humanId}
      </span>

      <StatusBadge status={item.status} />

      <div className="flex-1 min-w-0">
        {editing ? (
          <input
            ref={inputRef}
            value={title}
            onChange={(e) => onTitleChange(e.target.value)}
            onBlur={() => {
              onTitleCommit();
              setEditing(false);
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                onTitleCommit();
                setEditing(false);
              } else if (e.key === 'Escape') {
                onTitleChange(item.title);
                setEditing(false);
              }
            }}
            className="w-full rounded-md border border-accent bg-background px-2 py-1 text-base font-semibold text-white outline-none"
            placeholder="Card title"
          />
        ) : (
          <button
            type="button"
            onClick={() => setEditing(true)}
            className={cn(
              'w-full truncate rounded-md px-2 py-1 text-left text-base font-semibold text-white transition-colors',
              'hover:bg-edge/50',
            )}
            title="Click to edit title"
          >
            {title || <span className="text-muted italic">Untitled</span>}
          </button>
        )}
      </div>

      {item.status !== 'done' && (
        <button
          onClick={onComplete}
          className="flex items-center gap-1.5 rounded-md bg-emerald-500/10 px-2.5 py-1 text-xs font-medium text-emerald-300 transition-colors hover:bg-emerald-500/20"
          title="Mark as complete"
        >
          <CheckCircle size={12} />
          Complete
        </button>
      )}

      <ModalCloseButton onClick={onClose} />
    </div>
  );
}
