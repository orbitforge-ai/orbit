import { useState } from "react";
import { Bot, User, ChevronRight, Layers } from "lucide-react";
import { DisplayMessage } from "./types";
import { TextBlock } from "./TextBlock";
import { ThinkingBlock } from "./ThinkingBlock";
import { ToolUseBlock } from "./ToolUseBlock";
import { TypingIndicator } from "./StreamingCursor";

function formatTimestamp(iso: string): string {
  const d = new Date(iso);
  const now = new Date();
  const diffMs = now.getTime() - d.getTime();
  const diffDays = Math.floor(diffMs / 86400000);

  const time = d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });

  if (diffDays === 0) return time;
  if (diffDays === 1) return `Yesterday ${time}`;
  if (diffDays < 7) return `${d.toLocaleDateString([], { weekday: "short" })} ${time}`;
  return `${d.toLocaleDateString([], { month: "short", day: "numeric" })} ${time}`;
}

interface MessageBubbleProps {
  message: DisplayMessage;
}

export function MessageBubble({ message }: MessageBubbleProps) {
  const isUser = message.role === "user";
  const [expanded, setExpanded] = useState(false);

  // Summary message — collapsed by default, expandable
  if (message.isSummary) {
    return (
      <div className="flex flex-col items-center my-2">
        <button
          onClick={() => setExpanded(!expanded)}
          className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-[#1a1d27] border border-[#2a2d3e] text-[#64748b] hover:text-[#94a3b8] hover:border-[#4a4d6e] transition-colors text-xs"
        >
          <Layers size={12} />
          <span>Earlier conversation summarized</span>
          <ChevronRight
            size={12}
            className={`transition-transform ${expanded ? "rotate-90" : ""}`}
          />
        </button>
        {expanded && (
          <div className="mt-2 w-full max-w-[85%]">
            <div className="rounded-xl px-4 py-3 bg-[#1a1d27] border border-[#2a2d3e] opacity-75">
              {message.blocks.map((block, i) => {
                if (block.kind === "text") {
                  const text = block.text.replace(/^\[Conversation Summary\]\n?/, "");
                  return <TextBlock key={i} text={text} isStreaming={false} />;
                }
                return null;
              })}
            </div>
          </div>
        )}
      </div>
    );
  }

  // Compacted message — dimmed and collapsed
  if (message.isCompacted) {
    return (
      <div className={`flex gap-3 opacity-40 ${isUser ? "flex-row-reverse" : ""}`}>
        <div
          className={`shrink-0 w-5 h-5 rounded-full flex items-center justify-center mt-0.5 ${
            isUser
              ? "bg-[#6366f1]/10 text-[#818cf8]"
              : "bg-[#1a1d27] text-[#64748b] border border-[#2a2d3e]"
          }`}
        >
          {isUser ? <User size={10} /> : <Bot size={10} />}
        </div>
        <button
          onClick={() => setExpanded(!expanded)}
          className="min-w-0 max-w-[85%] text-left"
        >
          {expanded ? (
            <div className="rounded-lg px-3 py-2 bg-[#1a1d27]/50 border border-[#2a2d3e]/50 space-y-1">
              {message.blocks.map((block, i) => {
                if (block.kind === "text")
                  return (
                    <p key={i} className="text-xs text-[#64748b] leading-relaxed">
                      {block.text}
                    </p>
                  );
                return null;
              })}
            </div>
          ) : (
            <span className="text-[11px] text-[#4a4d6e] italic truncate block max-w-[300px]">
              {message.blocks.find((b) => b.kind === "text")?.kind === "text"
                ? (message.blocks.find((b) => b.kind === "text") as { kind: "text"; text: string })
                    .text.slice(0, 80) + "..."
                : "[message]"}
            </span>
          )}
        </button>
      </div>
    );
  }

  return (
    <div className={`flex gap-3 ${isUser ? "flex-row-reverse" : ""}`}>
      {/* Avatar */}
      <div
        className={`shrink-0 w-7 h-7 rounded-full flex items-center justify-center mt-0.5 ${
          isUser
            ? "bg-[#6366f1]/20 text-[#818cf8]"
            : "bg-[#1a1d27] text-[#64748b] border border-[#2a2d3e]"
        }`}
      >
        {isUser ? <User size={14} /> : <Bot size={14} />}
      </div>

      {/* Bubble */}
      <div
        className={`min-w-0 max-w-[85%] rounded-xl px-4 py-3 space-y-2 ${
          isUser
            ? "bg-[#6366f1]/15 border border-[#6366f1]/30"
            : "bg-[#1a1d27] border border-[#2a2d3e]"
        }`}
      >
        {message.blocks.map((block, i) => {
          switch (block.kind) {
            case "text":
              return <TextBlock key={i} text={block.text} isStreaming={block.isStreaming} />;
            case "thinking":
              return <ThinkingBlock key={i} thinking={block.thinking} />;
            case "tool_call":
              return (
                <ToolUseBlock
                  key={i}
                  name={block.name}
                  input={block.input}
                  result={block.result}
                />
              );
            case "image":
              return (
                <img
                  key={i}
                  src={`data:${block.mediaType};base64,${block.data}`}
                  alt="Attached image"
                  className="max-w-full max-h-[300px] rounded-lg object-contain"
                />
              );
          }
        })}
        {message.blocks.length === 0 && message.isStreaming && (
          <TypingIndicator />
        )}
      </div>

      {/* Timestamp — shown below the bubble, aligned to the side */}
      {message.timestamp && !message.isStreaming && (
        <div className="self-end mb-0.5 shrink-0">
          <span className="text-[10px] text-[#4a4d6e] tabular-nums">
            {formatTimestamp(message.timestamp)}
          </span>
        </div>
      )}
    </div>
  );
}
