import { useEffect, useRef, useState } from "react";
import { ChevronDown } from "lucide-react";
import { cn } from "../lib/cn";
import { LogLine } from "../types";

interface TerminalPaneProps {
  lines: LogLine[];
  className?: string;
  /** If true, auto-scroll to bottom unless user has scrolled up */
  live?: boolean;
}

function ansiToSpans(text: string): string {
  // Basic ANSI color escape sequence renderer
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/\x1b\[(\d+(?:;\d+)*)m/g, (_, codes) => {
      const parts = codes.split(";").map(Number);
      const styles: string[] = [];
      for (const code of parts) {
        if (code === 0) return "</span><span>"; // reset
        if (code === 1) styles.push("font-weight:bold");
        if (code === 31) styles.push("color:#f87171"); // red
        if (code === 32) styles.push("color:#4ade80"); // green
        if (code === 33) styles.push("color:#fbbf24"); // yellow
        if (code === 34) styles.push("color:#60a5fa"); // blue
        if (code === 35) styles.push("color:#a78bfa"); // magenta
        if (code === 36) styles.push("color:#34d399"); // cyan
        if (code === 37) styles.push("color:#e2e8f0"); // white
        if (code === 90) styles.push("color:#64748b"); // bright black (gray)
      }
      return styles.length
        ? `<span style="${styles.join(";")}">`
        : "<span>";
    })
    .replace(/\x1b\[\d*[A-Z]/g, ""); // strip other escape sequences
}

export function TerminalPane({ lines, className, live = false }: TerminalPaneProps) {
  const bottomRef = useRef<HTMLDivElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [atBottom, setAtBottom] = useState(true);

  // Auto-scroll when new lines arrive, if user hasn't scrolled up
  useEffect(() => {
    if (live && atBottom && bottomRef.current) {
      bottomRef.current.scrollIntoView({ behavior: "instant" });
    }
  }, [lines, live, atBottom]);

  function handleScroll() {
    const el = containerRef.current;
    if (!el) return;
    const isAtBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 20;
    setAtBottom(isAtBottom);
  }

  function scrollToBottom() {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
    setAtBottom(true);
  }

  return (
    <div className={cn("relative flex flex-col h-full", className)}>
      <div
        ref={containerRef}
        onScroll={handleScroll}
        className="flex-1 overflow-y-auto bg-inset rounded-lg p-3 font-mono text-xs leading-5 text-primary select-text"
      >
        {lines.length === 0 ? (
          <p className="text-muted italic">No output yet…</p>
        ) : (
          lines.map((line, i) => (
            <div
              key={i}
              className={cn(
                "whitespace-pre-wrap break-all",
                line.stream === "stderr" && "text-failure"
              )}
              dangerouslySetInnerHTML={{ __html: ansiToSpans(line.line) }}
            />
          ))
        )}
        <div ref={bottomRef} />
      </div>

      {/* Scroll-to-bottom button */}
      {!atBottom && (
        <button
          onClick={scrollToBottom}
          className="absolute bottom-4 right-4 flex items-center gap-1.5 px-2.5 py-1.5 rounded-md bg-edge hover:bg-edge-hover text-white text-xs transition-colors shadow-lg"
        >
          <ChevronDown size={12} />
          Latest
        </button>
      )}
    </div>
  );
}
