import { useEffect, useRef, useState, useMemo } from "react";
import { ArrowDown } from "lucide-react";
import { ChatMessage } from "../../types";
import { useLiveRunStore } from "../../store/liveRunStore";
import {
  onAgentLlmChunk,
  onAgentContentBlock,
  onAgentToolResult,
  onAgentIteration,
} from "../../events/runEvents";
import { DisplayMessage } from "./types";
import { chatMessagesToDisplay } from "./utils";
import { MessageBubble } from "./MessageBubble";

interface ChatViewProps {
  /** Static messages to display (history mode for completed runs) */
  messages?: ChatMessage[];
  /** Run ID to subscribe to live events (live mode for active runs) */
  liveRunId?: string;
  /** CSS class for the outer container */
  className?: string;
}

export function ChatView({ messages, liveRunId, className = "" }: ChatViewProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const store = useLiveRunStore();

  // History mode: convert static messages to display format
  const historyMessages = useMemo(() => {
    if (!messages) return [];
    return chatMessagesToDisplay(messages);
  }, [messages]);

  // Live mode: get display messages from the store
  const liveRun = liveRunId ? store.activeRuns[liveRunId] : undefined;
  const liveMessages = liveRun?.agentLoopState?.displayMessages ?? [];

  // Subscribe to live events
  useEffect(() => {
    if (!liveRunId) return;

    const unsubs: Promise<() => void>[] = [];

    unsubs.push(
      onAgentLlmChunk((payload) => {
        if (payload.runId === liveRunId) {
          store.appendTextDelta(liveRunId, payload.delta, payload.iteration);
        }
      })
    );

    unsubs.push(
      onAgentContentBlock((payload) => {
        if (payload.runId === liveRunId) {
          store.addContentBlock(liveRunId, payload.block, payload.iteration);
        }
      })
    );

    unsubs.push(
      onAgentToolResult((payload) => {
        if (payload.runId === liveRunId) {
          store.addToolResult(
            liveRunId,
            payload.toolUseId,
            payload.content,
            payload.isError
          );
        }
      })
    );

    unsubs.push(
      onAgentIteration((payload) => {
        if (payload.runId === liveRunId) {
          store.handleIteration(
            liveRunId,
            payload.iteration,
            payload.action,
            payload.totalTokens
          );
        }
      })
    );

    return () => {
      unsubs.forEach((p) => p.then((unsub) => unsub()));
    };
  }, [liveRunId]);

  // Determine which messages to render
  const displayMessages: DisplayMessage[] =
    liveRunId && liveMessages.length > 0 ? liveMessages : historyMessages;

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

  const showScrollButton = !autoScroll;

  return (
    <div className={`relative flex flex-col ${className}`}>
      <div
        ref={scrollRef}
        onScroll={handleScroll}
        className="flex-1 overflow-y-auto p-4 space-y-4"
      >
        {displayMessages.length === 0 && (
          <div className="text-center text-[#64748b] text-sm py-12">
            No messages yet.
          </div>
        )}
        {displayMessages.map((msg) => (
          <MessageBubble key={msg.id} message={msg} />
        ))}
      </div>

      {/* Scroll to bottom button */}
      {showScrollButton && (
        <button
          onClick={() => {
            if (scrollRef.current) {
              scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
              setAutoScroll(true);
            }
          }}
          className="absolute bottom-4 right-4 p-2 rounded-full bg-[#1a1d27] border border-[#2a2d3e] text-[#64748b] hover:text-white hover:border-[#6366f1] shadow-lg transition-colors"
        >
          <ArrowDown size={16} />
        </button>
      )}
    </div>
  );
}
