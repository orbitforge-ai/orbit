import { Loader2 } from 'lucide-react';

function StreamingStatusPill({ label }: { label: string }) {
  return (
    <div className="inline-flex items-center gap-2 rounded-full border border-accent/20 bg-accent/10 px-2.5 py-1 text-[11px] text-accent-light shadow-[0_6px_18px_rgba(15,23,42,0.18)]">
      <span className="flex h-4 w-4 items-center justify-center rounded-full bg-accent/15 text-accent-hover">
        <Loader2 size={10} className="animate-spin" />
      </span>
      <span className="font-medium tracking-[0.01em]">{label}</span>
      <span className="flex items-center gap-1">
        <span className="typing-dot h-1.5 w-1.5" />
        <span className="typing-dot h-1.5 w-1.5 [animation-delay:160ms]" />
        <span className="typing-dot h-1.5 w-1.5 [animation-delay:320ms]" />
      </span>
    </div>
  );
}

export function StreamingCursor() {
  return <StreamingStatusPill label="Responding" />;
}

export function TypingIndicator() {
  return (
    <div className="py-1">
      <StreamingStatusPill label="Thinking" />
    </div>
  );
}
