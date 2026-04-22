import { Flag } from 'lucide-react';
import { cn } from '../../../lib/cn';

const CONFIG: Record<number, { label: string; color: string }> = {
  0: { label: 'Low', color: 'text-muted' },
  1: { label: 'Medium', color: 'text-secondary' },
  2: { label: 'High', color: 'text-orange-400' },
  3: { label: 'Urgent', color: 'text-red-400' },
};

interface Props {
  priority: number;
  className?: string;
  compact?: boolean;
}

export function PriorityBadge({ priority, className, compact }: Props) {
  const cfg = CONFIG[priority] ?? CONFIG[1];
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1 text-xs font-medium',
        cfg.color,
        className,
      )}
      title={`${cfg.label} priority`}
    >
      <Flag size={compact ? 11 : 12} />
      {!compact && cfg.label}
    </span>
  );
}

export const PRIORITY_OPTIONS = [
  { value: '0', label: 'Low' },
  { value: '1', label: 'Medium' },
  { value: '2', label: 'High' },
  { value: '3', label: 'Urgent' },
];
