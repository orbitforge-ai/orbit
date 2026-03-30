import { useState } from "react";
import { Brain, ChevronRight } from "lucide-react";

interface ThinkingBlockProps {
  thinking: string;
}

export function ThinkingBlock({ thinking }: ThinkingBlockProps) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="rounded-lg border border-edge bg-background overflow-hidden">
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-2 w-full px-3 py-2 text-xs text-muted hover:text-secondary transition-colors"
      >
        <ChevronRight
          size={12}
          className={`transition-transform ${expanded ? "rotate-90" : ""}`}
        />
        <Brain size={12} />
        <span>Thinking</span>
        {!expanded && (
          <span className="truncate ml-1 opacity-50">
            {thinking.slice(0, 80)}
            {thinking.length > 80 ? "..." : ""}
          </span>
        )}
      </button>
      {expanded && (
        <div className="px-3 pb-3 text-xs text-muted whitespace-pre-wrap leading-relaxed border-t border-edge">
          {thinking}
        </div>
      )}
    </div>
  );
}
