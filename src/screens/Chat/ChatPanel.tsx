import { useEffect, useRef, useState, useMemo, useCallback } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowDown } from "lucide-react";
import { chatApi } from "../../api/chat";
import { ContentBlock } from "../../types";
import { DisplayMessage, DisplayBlock } from "../../components/chat/types";
import { chatMessagesToDisplay } from "../../components/chat/utils";
import { MessageBubble } from "../../components/chat/MessageBubble";
import { ChatInput } from "./ChatInput";
import { ContextGauge } from "../../components/chat/ContextGauge";
import {
  onAgentLlmChunk,
  onAgentContentBlock,
  onAgentToolResult,
  onAgentIteration,
} from "../../events/runEvents";

interface ChatPanelProps {
  sessionId: string;
}

let msgId = 0;

export function ChatPanel({ sessionId }: ChatPanelProps) {
  const queryClient = useQueryClient();
  const scrollRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const [streaming, setStreaming] = useState(false);
  const [streamMessages, setStreamMessages] = useState<DisplayMessage[]>([]);

  const streamId = `chat:${sessionId}`;

  // Load messages from DB
  const { data: dbMessages } = useQuery({
    queryKey: ["chat-messages", sessionId],
    queryFn: () => chatApi.getMessages(sessionId),
    refetchInterval: streaming ? false : 10_000,
  });

  const historyMessages = useMemo(() => {
    if (!dbMessages) return [];
    return chatMessagesToDisplay(dbMessages);
  }, [dbMessages]);

  // Combine history + streaming messages
  const displayMessages = streaming ? streamMessages : historyMessages;

  // Subscribe to streaming events
  useEffect(() => {
    const unsubs: Promise<() => void>[] = [];

    unsubs.push(
      onAgentLlmChunk((payload) => {
        if (payload.runId !== streamId) return;
        setStreamMessages((prev) => {
          const msgs = [...prev];
          let last = msgs[msgs.length - 1];
          if (!last || last.role !== "assistant" || !last.isStreaming) {
            last = { id: `stream-${++msgId}`, role: "assistant", blocks: [], isStreaming: true };
            msgs.push(last);
          } else {
            last = { ...last };
            msgs[msgs.length - 1] = last;
          }

          const blocks = [...last.blocks];
          const lastBlock = blocks[blocks.length - 1];
          if (lastBlock && lastBlock.kind === "text" && lastBlock.isStreaming) {
            blocks[blocks.length - 1] = { ...lastBlock, text: lastBlock.text + payload.delta };
          } else {
            blocks.push({ kind: "text", text: payload.delta, isStreaming: true });
          }
          last.blocks = blocks;

          return msgs;
        });
      })
    );

    unsubs.push(
      onAgentContentBlock((payload) => {
        if (payload.runId !== streamId) return;
        setStreamMessages((prev) => {
          const msgs = [...prev];
          let last = msgs[msgs.length - 1];
          if (!last || last.role !== "assistant") return prev;
          last = { ...last };
          msgs[msgs.length - 1] = last;

          const blocks = [...last.blocks];
          // Finalize any streaming text block
          const lastBlock = blocks[blocks.length - 1];
          if (lastBlock && lastBlock.kind === "text" && lastBlock.isStreaming) {
            blocks[blocks.length - 1] = { ...lastBlock, isStreaming: false };
          }

          if (payload.block.type === "thinking") {
            blocks.push({ kind: "thinking", thinking: payload.block.thinking });
          } else if (payload.block.type === "tool_use") {
            blocks.push({
              kind: "tool_call",
              id: payload.block.id,
              name: payload.block.name,
              input: payload.block.input,
            });
          }
          last.blocks = blocks;

          return msgs;
        });
      })
    );

    unsubs.push(
      onAgentToolResult((payload) => {
        if (payload.runId !== streamId) return;
        setStreamMessages((prev) => {
          const msgs = [...prev];
          for (let i = msgs.length - 1; i >= 0; i--) {
            const msg = msgs[i];
            if (msg.role !== "assistant") continue;
            for (let j = msg.blocks.length - 1; j >= 0; j--) {
              const block = msg.blocks[j];
              if (block.kind === "tool_call" && block.id === payload.toolUseId) {
                const updatedMsg = { ...msg, blocks: [...msg.blocks] };
                updatedMsg.blocks[j] = {
                  ...block,
                  result: { content: payload.content, isError: payload.isError },
                };
                msgs[i] = updatedMsg;
                return msgs;
              }
            }
          }
          return prev;
        });
      })
    );

    unsubs.push(
      onAgentIteration((payload) => {
        if (payload.runId !== streamId) return;
        if (payload.action === "finished") {
          setStreaming(false);
          // Finalize streaming blocks
          setStreamMessages((prev) => {
            const msgs = [...prev];
            const last = msgs[msgs.length - 1];
            if (last && last.isStreaming) {
              const updated = { ...last, isStreaming: false, blocks: [...last.blocks] };
              const lastBlock = updated.blocks[updated.blocks.length - 1];
              if (lastBlock && lastBlock.kind === "text" && lastBlock.isStreaming) {
                updated.blocks[updated.blocks.length - 1] = { ...lastBlock, isStreaming: false };
              }
              msgs[msgs.length - 1] = updated;
            }
            return msgs;
          });
          // Refetch from DB for consistency
          queryClient.invalidateQueries({ queryKey: ["chat-messages", sessionId] });
          queryClient.invalidateQueries({ queryKey: ["chat-sessions"] });
        }
      })
    );

    return () => {
      unsubs.forEach((p) => p.then((unsub) => unsub()));
    };
  }, [streamId, sessionId]);

  // Auto-scroll
  useEffect(() => {
    if (autoScroll && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [displayMessages, autoScroll]);

  function handleScroll() {
    if (!scrollRef.current) return;
    const { scrollTop, scrollHeight, clientHeight } = scrollRef.current;
    setAutoScroll(scrollHeight - scrollTop - clientHeight < 50);
  }

  const handleSend = useCallback(
    async (content: ContentBlock[]) => {
      // Optimistically add user message to stream view
      const userMsg: DisplayMessage = {
        id: `user-${++msgId}`,
        role: "user",
        blocks: content.map((block): DisplayBlock => {
          if (block.type === "text") return { kind: "text", text: block.text, isStreaming: false };
          if (block.type === "image")
            return { kind: "image", mediaType: block.media_type, data: block.data };
          return { kind: "text", text: "[attachment]", isStreaming: false };
        }),
        isStreaming: false,
        timestamp: new Date().toISOString(),
      };

      // Add user message + empty streaming assistant placeholder
      const assistantPlaceholder: DisplayMessage = {
        id: `assistant-${++msgId}`,
        role: "assistant",
        blocks: [],
        isStreaming: true,
      };
      setStreamMessages([...historyMessages, userMsg, assistantPlaceholder]);
      setStreaming(true);

      try {
        await chatApi.sendMessage(sessionId, content);
      } catch (err) {
        console.error("Failed to send message:", err);
        setStreaming(false);
      }
    },
    [sessionId, historyMessages]
  );

  const showScrollBtn = !autoScroll;

  return (
    <div className="flex flex-col h-full">
      {/* Messages */}
      <div className="relative flex-1 min-h-0">
        <div
          ref={scrollRef}
          onScroll={handleScroll}
          className="h-full overflow-y-auto overflow-x-hidden p-4 space-y-4"
        >
          {displayMessages.length === 0 && (
            <div className="flex items-center justify-center h-full text-muted text-sm">
              Send a message to start the conversation.
            </div>
          )}
          {displayMessages.map((msg) => (
            <MessageBubble key={msg.id} message={msg} />
          ))}
        </div>

        {showScrollBtn && (
          <button
            onClick={() => {
              if (scrollRef.current) {
                scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
                setAutoScroll(true);
              }
            }}
            className="absolute bottom-3 right-3 p-2 rounded-full bg-surface border border-edge text-muted hover:text-white hover:border-accent shadow-lg transition-colors"
          >
            <ArrowDown size={14} />
          </button>
        )}
      </div>

      {/* Input */}
      <ChatInput
        onSend={handleSend}
        disabled={streaming}
        contextGauge={
          <ContextGauge
            sessionId={sessionId}
            onCompacted={() => {
              queryClient.invalidateQueries({ queryKey: ["chat-messages", sessionId] });
            }}
          />
        }
      />
    </div>
  );
}
