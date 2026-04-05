import { getToolVisual } from '../chat/toolVisuals';

interface ToolBadgeProps {
  toolName: string;
}

export function ToolBadge({ toolName }: ToolBadgeProps) {
  const { Icon, colorClass } = getToolVisual(toolName);

  return (
    <div
      className="absolute -top-3 -right-3 w-7 h-7 rounded-full bg-surface border border-edge flex items-center justify-center shadow-lg"
      title={toolName}
    >
      <Icon
        size={14}
        className={`${colorClass} shrink-0`}
        style={{ animation: 'tool-badge-spin 1.8s linear infinite' }}
      />
    </div>
  );
}
