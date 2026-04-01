import { useEffect, useRef, useState, useMemo, useCallback } from 'react';
import { useInfiniteQuery, useQueryClient } from '@tanstack/react-query';
import { useVirtualizer } from '@tanstack/react-virtual';
import { ArrowDown, Loader2 } from 'lucide-react';
import { chatApi } from '../../api/chat';
import { ContentBlock } from '../../types';
import { DisplayMessage, DisplayBlock } from '../../components/chat/types';
import { chatMessagesToDisplay } from '../../components/chat/utils';
import { MessageBubble } from '../../components/chat/MessageBubble';
import { ChatInput } from './ChatInput';
import { ContextGauge } from '../../components/chat/ContextGauge';
import {
  onAgentLlmChunk,
  onAgentContentBlock,
  onAgentToolResult,
  onAgentIteration,
} from '../../events/runEvents';
import { onPermissionRequest, onPermissionCancelled } from '../../events/permissionEvents';
import { usePermissionStore } from '../../store/permissionStore';

const PAGE_SIZE = 50;

interface ChatPanelProps {
  sessionId: string;
}

let msgId = 0;

export function ChatPanel({ sessionId }: ChatPanelProps) {
  const queryClient = useQueryClient();
  const parentRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const [streaming, setStreaming] = useState(false);
  const [streamMessages, setStreamMessages] = useState<DisplayMessage[]>([]);

  // Scroll position preservation for loading older messages
  const prevScrollHeightRef = useRef(0);
  const prevScrollTopRef = useRef(0);
  const isLoadingOlderRef = useRef(false);

  const streamId = `chat:${sessionId}`;

  // Load messages from DB with pagination
  const { data, fetchNextPage, hasNextPage, isFetchingNextPage } = useInfiniteQuery({
    queryKey: ['chat-messages', sessionId],
    queryFn: async ({ pageParam = 0 }) => {
      return chatApi.getMessagesPaginated(sessionId, PAGE_SIZE, pageParam);
    },
    initialPageParam: 0,
    getNextPageParam: (lastPage, allPages) => {
      if (!lastPage.hasMore) return undefined;
      const totalLoaded = allPages.reduce((sum, p) => sum + p.messages.length, 0);
      return totalLoaded;
    },
    refetchInterval: streaming ? false : 10_000,
    refetchOnWindowFocus: false,
    staleTime: 30_000,
    gcTime: 10 * 60_000,
  });

  // Flatten pages: pages[0]=newest, pages[N]=oldest → reverse so oldest first
  const allDbMessages = useMemo(() => {
    if (!data?.pages) return [];
    const reversed = [...data.pages].reverse();
    const all = reversed.flatMap((page) => page.messages);
    // Deduplicate by created_at+role in case pages overlap from new messages arriving
    const seen = new Set<string>();
    return all.filter((msg) => {
      const key = `${msg.created_at}:${msg.role}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
  }, [data]);

  const historyMessages = useMemo(() => {
    return chatMessagesToDisplay(allDbMessages);
  }, [allDbMessages]);

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
          if (!last || last.role !== 'assistant' || !last.isStreaming) {
            last = { id: `stream-${++msgId}`, role: 'assistant', blocks: [], isStreaming: true };
            msgs.push(last);
          } else {
            last = { ...last };
            msgs[msgs.length - 1] = last;
          }

          const blocks = [...last.blocks];
          const lastBlock = blocks[blocks.length - 1];
          if (lastBlock && lastBlock.kind === 'text' && lastBlock.isStreaming) {
            blocks[blocks.length - 1] = { ...lastBlock, text: lastBlock.text + payload.delta };
          } else {
            blocks.push({ kind: 'text', text: payload.delta, isStreaming: true });
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
          if (!last || last.role !== 'assistant') return prev;
          last = { ...last };
          msgs[msgs.length - 1] = last;

          const blocks = [...last.blocks];
          // Finalize any streaming text block
          const lastBlock = blocks[blocks.length - 1];
          if (lastBlock && lastBlock.kind === 'text' && lastBlock.isStreaming) {
            blocks[blocks.length - 1] = { ...lastBlock, isStreaming: false };
          }

          if (payload.block.type === 'thinking') {
            blocks.push({ kind: 'thinking', thinking: payload.block.thinking });
          } else if (payload.block.type === 'tool_use') {
            blocks.push({
              kind: 'tool_call',
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
            if (msg.role !== 'assistant') continue;
            for (let j = msg.blocks.length - 1; j >= 0; j--) {
              const block = msg.blocks[j];
              if (block.kind === 'tool_call' && block.id === payload.toolUseId) {
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
        if (payload.action === 'finished') {
          setStreaming(false);
          // Finalize streaming blocks
          setStreamMessages((prev) => {
            const msgs = [...prev];
            const last = msgs[msgs.length - 1];
            if (last && last.isStreaming) {
              const updated = { ...last, isStreaming: false, blocks: [...last.blocks] };
              const lastBlock = updated.blocks[updated.blocks.length - 1];
              if (lastBlock && lastBlock.kind === 'text' && lastBlock.isStreaming) {
                updated.blocks[updated.blocks.length - 1] = { ...lastBlock, isStreaming: false };
              }
              msgs[msgs.length - 1] = updated;
            }
            return msgs;
          });
          // Refetch from DB for consistency
          queryClient.invalidateQueries({ queryKey: ['chat-messages', sessionId] });
          queryClient.invalidateQueries({ queryKey: ['chat-sessions'] });
        }
      })
    );

    // Permission request: inject a permission_prompt block into the stream
    unsubs.push(
      onPermissionRequest((payload) => {
        if (payload.runId !== streamId && payload.sessionId !== sessionId) return;
        usePermissionStore.getState().addRequest(payload);
        setStreamMessages((prev) => {
          const msgs = [...prev];
          let last = msgs[msgs.length - 1];
          if (!last || last.role !== 'assistant') {
            last = { id: `stream-${++msgId}`, role: 'assistant', blocks: [], isStreaming: true };
            msgs.push(last);
          } else {
            last = { ...last };
            msgs[msgs.length - 1] = last;
          }
          last.blocks = [
            ...last.blocks,
            {
              kind: 'permission_prompt' as const,
              requestId: payload.requestId,
              toolName: payload.toolName,
              toolInput: payload.toolInput,
              riskLevel: payload.riskLevel,
              riskDescription: payload.riskDescription,
              suggestedPattern: payload.suggestedPattern,
            },
          ];
          return msgs;
        });
      })
    );

    // Permission cancelled: remove the prompt block
    unsubs.push(
      onPermissionCancelled((payload) => {
        usePermissionStore.getState().removeRequest(payload.requestId);
      })
    );

    return () => {
      unsubs.forEach((p) => p.then((unsub) => unsub()));
    };
  }, [streamId, sessionId]);

  // Scroll position preservation after loading older messages
  useEffect(() => {
    if (!isLoadingOlderRef.current || !parentRef.current) return;
    requestAnimationFrame(() => {
      if (!parentRef.current) return;
      const el = parentRef.current;
      const newScrollHeight = el.scrollHeight;
      const heightDiff = newScrollHeight - prevScrollHeightRef.current;
      el.scrollTop = prevScrollTopRef.current + heightDiff;
      isLoadingOlderRef.current = false;
    });
  }, [allDbMessages]);

  const virtualizer = useVirtualizer({
    count: displayMessages.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 120,
    overscan: 5,
  });

  // Auto-scroll — use virtualizer.scrollToIndex so it accounts for measured sizes
  useEffect(() => {
    if (autoScroll && displayMessages.length > 0 && !isLoadingOlderRef.current) {
      // requestAnimationFrame lets the virtualizer measure before we scroll
      requestAnimationFrame(() => {
        virtualizer.scrollToIndex(displayMessages.length - 1, { align: 'end' });
      });
    }
  }, [displayMessages, autoScroll]);

  const handleLoadOlder = useCallback(() => {
    if (!parentRef.current || !hasNextPage || isFetchingNextPage) return;
    const el = parentRef.current;
    prevScrollHeightRef.current = el.scrollHeight;
    prevScrollTopRef.current = el.scrollTop;
    isLoadingOlderRef.current = true;
    fetchNextPage();
  }, [fetchNextPage, hasNextPage, isFetchingNextPage]);

  function handleScroll() {
    if (!parentRef.current) return;
    const { scrollTop, scrollHeight, clientHeight } = parentRef.current;
    setAutoScroll(scrollHeight - scrollTop - clientHeight < 50);

    // Load older messages when scrolled near top
    if (scrollTop < 200 && hasNextPage && !isFetchingNextPage) {
      handleLoadOlder();
    }
  }

  const handleSend = useCallback(
    async (content: ContentBlock[]) => {
      // Optimistically add user message to stream view
      const userMsg: DisplayMessage = {
        id: `user-${++msgId}`,
        role: 'user',
        blocks: content.map((block): DisplayBlock => {
          if (block.type === 'text') return { kind: 'text', text: block.text, isStreaming: false };
          if (block.type === 'image')
            return { kind: 'image', mediaType: block.media_type, data: block.data };
          return { kind: 'text', text: '[attachment]', isStreaming: false };
        }),
        isStreaming: false,
        timestamp: new Date().toISOString(),
      };

      // Add user message + empty streaming assistant placeholder
      const assistantPlaceholder: DisplayMessage = {
        id: `assistant-${++msgId}`,
        role: 'assistant',
        blocks: [],
        isStreaming: true,
      };
      setStreamMessages([...historyMessages, userMsg, assistantPlaceholder]);
      setStreaming(true);

      try {
        await chatApi.sendMessage(sessionId, content);
      } catch (err) {
        console.error('Failed to send message:', err);
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
          ref={parentRef}
          onScroll={handleScroll}
          className="h-full overflow-y-auto overflow-x-hidden"
        >
          {isFetchingNextPage && (
            <div className="flex items-center justify-center gap-2 py-3">
              <Loader2 size={14} className="animate-spin text-muted" />
              <span className="text-muted text-xs">Loading older messages...</span>
            </div>
          )}

          {displayMessages.length === 0 ? (
            <div className="flex items-center justify-center h-full text-muted text-sm">
              Send a message to start the conversation.
            </div>
          ) : (
            <div
              style={{
                height: `${virtualizer.getTotalSize()}px`,
                width: '100%',
                position: 'relative',
              }}
            >
              {virtualizer.getVirtualItems().map((virtualRow) => {
                const msg = displayMessages[virtualRow.index];
                return (
                  <div
                    key={virtualRow.key}
                    data-index={virtualRow.index}
                    ref={virtualizer.measureElement}
                    style={{
                      position: 'absolute',
                      top: 0,
                      left: 0,
                      width: '100%',
                      transform: `translateY(${virtualRow.start}px)`,
                    }}
                    className="px-4 py-2"
                  >
                    <MessageBubble message={msg} />
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {showScrollBtn && (
          <button
            onClick={() => {
              if (parentRef.current) {
                parentRef.current.scrollTop = parentRef.current.scrollHeight;
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
              queryClient.invalidateQueries({ queryKey: ['chat-messages', sessionId] });
            }}
          />
        }
      />
    </div>
  );
}
