import { useCallback, useMemo, useState } from "react";
import { Bot, User, ChevronRight, Layers, ExternalLink } from "lucide-react";
import TurndownService from "turndown";
import { DisplayMessage } from "./types";
import { TextBlock } from "./TextBlock";
import { ThinkingBlock } from "./ThinkingBlock";
import { ToolUseBlock } from "./ToolUseBlock";
import { PermissionPrompt } from "./PermissionPrompt";
import { TypingIndicator } from "./StreamingCursor";
import { useUiStore } from "../../store/uiStore";

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
  agentId?: string;
}

export function MessageBubble({ message, agentId }: MessageBubbleProps) {
  const isUser = message.role === "user";
  const [expanded, setExpanded] = useState(false);

  const turndown = useMemo(() => {
    const td = new TurndownService({ headingStyle: "atx", codeBlockStyle: "fenced" });
    td.keep(["del"]);
    return td;
  }, []);

  const handleCopy = useCallback((e: React.ClipboardEvent) => {
    const selection = window.getSelection();
    if (!selection || selection.isCollapsed) return;

    const fragment = selection.getRangeAt(0).cloneContents();
    const wrapper = document.createElement("div");
    wrapper.appendChild(fragment);

    const markdown = turndown.turndown(wrapper.innerHTML).trim();
    if (!markdown) return;

    e.preventDefault();
    e.clipboardData.setData("text/plain", markdown);
  }, [turndown]);

  // Summary message — collapsed by default, expandable
  if (message.isSummary) {
    return (
      <div className="flex flex-col items-center my-2">
        <button
          onClick={() => setExpanded(!expanded)}
          className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-surface border border-edge text-muted hover:text-secondary hover:border-edge-hover transition-colors text-xs"
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
            <div className="rounded-xl px-4 py-3 bg-surface border border-edge opacity-75">
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
              ? "bg-accent/10 text-accent-hover"
              : "bg-surface text-muted border border-edge"
          }`}
        >
          {isUser ? <User size={10} /> : <Bot size={10} />}
        </div>
        <button
          onClick={() => setExpanded(!expanded)}
          className="min-w-0 max-w-[85%] text-left"
        >
          {expanded ? (
            <div className="rounded-lg px-3 py-2 bg-surface/50 border border-edge/50 space-y-1">
              {message.blocks.map((block, i) => {
                if (block.kind === "text")
                  return (
                    <p key={i} className="text-xs text-muted leading-relaxed">
                      {block.text}
                    </p>
                  );
                return null;
              })}
            </div>
          ) : (
            <span className="text-[11px] text-border-hover italic truncate block max-w-[300px]">
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

  const isBusSender = isUser && !!message.senderLabel;
  const navigate = useUiStore((s) => s.navigate);
  const selectRun = useUiStore((s) => s.selectRun);

  return (
    <div className={`flex gap-3 ${isUser && !isBusSender ? "flex-row-reverse" : ""}`}>
      {/* Avatar */}
      {isBusSender ? (
        <div className="flex items-center gap-1.5 shrink-0 mt-0.5">
          <div className="w-7 h-7 rounded-full flex items-center justify-center bg-blue-500/20 text-blue-400 border border-blue-500/30">
            <Bot size={14} />
          </div>
          <span className="text-[10px] text-blue-400 font-medium max-w-[100px] truncate">
            {message.senderLabel}
          </span>
        </div>
      ) : (
        <div
          className={`shrink-0 w-7 h-7 rounded-full flex items-center justify-center mt-0.5 ${
            isUser
              ? "bg-accent/20 text-accent-hover"
              : "bg-surface text-muted border border-edge"
          }`}
        >
          {isUser ? <User size={14} /> : <Bot size={14} />}
        </div>
      )}

      {/* Bubble */}
      <div
        onCopy={handleCopy}
        className={`min-w-0 max-w-[85%] rounded-xl px-4 py-3 space-y-2 overflow-hidden select-text ${
          isBusSender
            ? "bg-blue-500/10 border border-blue-500/20"
            : isUser
              ? "bg-accent/15 border border-accent/30"
              : "bg-surface border border-edge"
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
            case "permission_prompt":
              return (
                <PermissionPrompt
                  key={i}
                  requestId={block.requestId}
                  toolName={block.toolName}
                  toolInput={block.toolInput}
                  riskLevel={block.riskLevel}
                  riskDescription={block.riskDescription}
                  suggestedPattern={block.suggestedPattern}
                  agentId={agentId ?? ""}
                  resolved={block.resolved}
                />
              );
          }
        })}
        {message.blocks.length === 0 && message.isStreaming && (
          <TypingIndicator />
        )}
        {message.linkedRunId && !message.isStreaming && (
          <button
            onClick={() => {
              selectRun(message.linkedRunId!);
              navigate("history");
            }}
            className="flex items-center gap-1 text-[10px] text-accent-hover hover:text-white transition-colors mt-1"
          >
            <ExternalLink size={10} />
            View Run
          </button>
        )}
      </div>

      {/* Timestamp — shown below the bubble, aligned to the side */}
      {message.timestamp && !message.isStreaming && (
        <div className="self-end mb-0.5 shrink-0">
          <span className="text-[10px] text-border-hover tabular-nums">
            {formatTimestamp(message.timestamp)}
          </span>
        </div>
      )}
    </div>
  );
}
