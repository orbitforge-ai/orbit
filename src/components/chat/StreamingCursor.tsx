export function StreamingCursor() {
  return (
    <span className="inline-block w-2 h-4 ml-0.5 bg-[#6366f1] rounded-sm animate-pulse align-text-bottom" />
  );
}

export function TypingIndicator() {
  return (
    <div className="flex items-center gap-3 py-1">
      <div className="flex items-center gap-1">
        <span className="typing-dot" />
        <span className="typing-dot [animation-delay:160ms]" />
        <span className="typing-dot [animation-delay:320ms]" />
      </div>
      <span className="text-xs text-[#64748b]">Thinking...</span>
    </div>
  );
}
