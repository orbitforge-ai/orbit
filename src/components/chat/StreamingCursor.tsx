function StreamingStatusPill({ label }: { label: string }) {
  return (
    <div className="streaming-status-pill inline-flex items-center rounded-full border border-accent/20 bg-accent/10 px-3 py-1 text-[11px] text-accent-light shadow-[0_6px_18px_rgba(15,23,42,0.18)]">
      <span aria-hidden="true" className="streaming-status-pill__orbit" />
      <span className="relative z-10 font-medium tracking-[0.01em]">{label}</span>
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
