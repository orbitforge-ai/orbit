import { Bot, User } from "lucide-react";
import { DisplayMessage } from "./types";
import { TextBlock } from "./TextBlock";
import { ThinkingBlock } from "./ThinkingBlock";
import { ToolUseBlock } from "./ToolUseBlock";
import { TypingIndicator } from "./StreamingCursor";

interface MessageBubbleProps {
  message: DisplayMessage;
}

export function MessageBubble({ message }: MessageBubbleProps) {
  const isUser = message.role === "user";

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
    </div>
  );
}
