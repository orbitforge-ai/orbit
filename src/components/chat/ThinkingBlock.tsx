import { Brain } from 'lucide-react';

interface ThinkingBlockProps {
  thinking: string;
  selected: boolean;
  disabled?: boolean;
  onClick: () => void;
}

export function ThinkingBlock({
  thinking,
  selected,
  disabled = false,
  onClick,
}: ThinkingBlockProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={`inline-flex max-w-full items-center gap-1.5 rounded-full border border-dashed px-2.5 py-1 text-[11px] transition-colors ${
        selected
          ? 'border-accent/50 bg-accent/10 text-accent-light shadow-[0_0_0_1px_rgba(59,130,246,0.12)]'
          : 'border-edge-hover bg-background text-muted hover:border-accent/35 hover:text-secondary'
      } ${disabled ? 'cursor-default opacity-90' : ''}`}
      title={thinking}
    >
      <Brain size={11} className="shrink-0 text-accent-hover" />
      <span className="font-medium">Thinking</span>
      <span className="max-w-[220px] truncate opacity-70">
        {thinking.slice(0, 48)}
        {thinking.length > 48 ? '...' : ''}
      </span>
    </button>
  );
}

export function ThinkingDetailPanel({ thinking }: { thinking: string }) {
  return (
    <div className="rounded-lg border border-dashed border-accent/30 bg-accent/5 overflow-hidden">
      <div className="flex items-center gap-1.5 px-3 py-2 border-b border-accent/15 text-[11px] text-accent-light">
        <Brain size={11} className="shrink-0" />
        <span className="font-medium">Thinking</span>
      </div>
      <div className="px-3 py-3 text-xs text-muted whitespace-pre-wrap leading-relaxed">
        {thinking}
      </div>
    </div>
  );
}
