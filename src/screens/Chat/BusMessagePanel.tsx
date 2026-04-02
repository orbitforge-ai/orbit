import { useEffect, useRef, useState, useMemo, useCallback } from 'react';
import { useInfiniteQuery, useQueryClient } from '@tanstack/react-query';
import { useVirtualizer } from '@tanstack/react-virtual';
import { ArrowDown, Loader2, MessageSquare } from 'lucide-react';
import { busApi } from '../../api/bus';
import { BusThreadMessage } from '../../types';
import { DisplayMessage } from '../../components/chat/types';
import { MessageBubble } from '../../components/chat/MessageBubble';
import { onBusMessageSent, onRunStateChanged } from '../../events/runEvents';

const PAGE_SIZE = 50;
const TERMINAL_STATES = new Set(['success', 'failure', 'cancelled', 'timed_out']);

interface BusMessagePanelProps {
  agentId: string;
}

function busMessagesToDisplay(messages: BusThreadMessage[]): DisplayMessage[] {
  const result: DisplayMessage[] = [];
  let id = 0;

  for (const msg of messages) {
    const payloadText = (() => {
      const p = msg.payload as unknown;
      if (typeof p === 'string') return p;
      if (typeof p === 'object' && p !== null && 'message' in p) {
        const m = (p as Record<string, unknown>).message;
        if (m != null) return String(m);
      }
      return JSON.stringify(p, null, 2);
    })();

    // Incoming message from sender agent
    result.push({
      id: `bus-in-${msg.id}-${++id}`,
      role: 'user',
      blocks: [{ kind: 'text', text: payloadText, isStreaming: false }],
      isStreaming: false,
      timestamp: msg.createdAt,
      senderLabel: msg.fromAgentName,
    });

    // Response from triggered run
    if (msg.triggeredRunState && TERMINAL_STATES.has(msg.triggeredRunState)) {
      result.push({
        id: `bus-out-${msg.id}-${++id}`,
        role: 'assistant',
        blocks: [
          {
            kind: 'text',
            text: msg.triggeredRunSummary || `Run ${msg.triggeredRunState}`,
            isStreaming: false,
          },
        ],
        isStreaming: false,
        timestamp: msg.createdAt,
        linkedRunId: msg.triggeredRunId ?? undefined,
      });
    } else if (msg.triggeredRunId) {
      // Run still in progress
      result.push({
        id: `bus-out-${msg.id}-${++id}`,
        role: 'assistant',
        blocks: [],
        isStreaming: true,
        linkedRunId: msg.triggeredRunId,
      });
    }
  }

  return result;
}

export function BusMessagePanel({ agentId }: BusMessagePanelProps) {
  const queryClient = useQueryClient();
  const parentRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);

  const prevScrollHeightRef = useRef(0);
  const prevScrollTopRef = useRef(0);
  const isLoadingOlderRef = useRef(false);

  // Load bus messages with pagination
  const { data, fetchNextPage, hasNextPage, isFetchingNextPage } = useInfiniteQuery({
    queryKey: ['bus-thread', agentId],
    queryFn: async ({ pageParam = 0 }) => {
      return busApi.getBusThread(agentId, PAGE_SIZE, pageParam);
    },
    initialPageParam: 0,
    getNextPageParam: (lastPage, allPages) => {
      if (!lastPage.hasMore) return undefined;
      const totalLoaded = allPages.reduce((sum, p) => sum + p.messages.length, 0);
      return totalLoaded;
    },
    refetchInterval: 10_000,
    refetchOnWindowFocus: false,
    staleTime: 5_000,
  });

  // Flatten pages (newest first from DB → reverse so oldest first)
  const allBusMessages = useMemo(() => {
    if (!data?.pages) return [];
    const reversed = [...data.pages].reverse();
    const all = reversed.flatMap((page) => page.messages);
    const seen = new Set<string>();
    return all.filter((msg) => {
      if (seen.has(msg.id)) return false;
      seen.add(msg.id);
      return true;
    });
  }, [data]);

  const displayMessages = useMemo(() => busMessagesToDisplay(allBusMessages), [allBusMessages]);

  // Real-time: new bus messages targeting this agent
  useEffect(() => {
    const unsub = onBusMessageSent((payload) => {
      if (payload.toAgentId === agentId) {
        queryClient.invalidateQueries({ queryKey: ['bus-thread', agentId] });
      }
    });
    return () => {
      unsub.then((fn) => fn()).catch(() => {});
    };
  }, [agentId, queryClient]);

  // Real-time: run state changes (to pick up finish_summary)
  useEffect(() => {
    const unsub = onRunStateChanged((payload) => {
      if (TERMINAL_STATES.has(payload.newState)) {
        queryClient.invalidateQueries({ queryKey: ['bus-thread', agentId] });
      }
    });
    return () => {
      unsub.then((fn) => fn()).catch(() => {});
    };
  }, [agentId, queryClient]);

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
  }, [allBusMessages]);

  const virtualizer = useVirtualizer({
    count: displayMessages.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 120,
    overscan: 5,
  });

  // Auto-scroll to bottom
  useEffect(() => {
    if (autoScroll && displayMessages.length > 0 && !isLoadingOlderRef.current) {
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

    if (scrollTop < 200 && hasNextPage && !isFetchingNextPage) {
      handleLoadOlder();
    }
  }

  const showScrollBtn = !autoScroll;

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="flex items-center gap-2 px-4 py-3 border-b border-edge">
        <MessageSquare size={14} className="text-blue-400" />
        <h3 className="text-sm font-semibold text-white">Agent Messages</h3>
        <span className="text-xs text-muted">{data?.pages?.[0]?.totalCount ?? 0} messages</span>
      </div>

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
            <div className="flex flex-col items-center justify-center h-full text-muted text-sm gap-2">
              <MessageSquare size={24} className="opacity-30" />
              <p>No agent messages yet.</p>
              <p className="text-xs">Messages from other agents will appear here.</p>
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
    </div>
  );
}
