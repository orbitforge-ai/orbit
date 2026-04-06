import { useEffect, useRef, useState, useMemo, useCallback } from 'react';
import { useInfiniteQuery, useQuery, useQueryClient } from '@tanstack/react-query';
import { useVirtualizer } from '@tanstack/react-virtual';
import { ArrowDown, Loader2 } from 'lucide-react';
import { chatApi } from '../../api/chat';
import { AgentIdentityConfig, ChatDraft, ContentBlock } from '../../types';
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
  onMessageReaction,
  onUserQuestion,
} from '../../events/runEvents';
import { onPermissionRequest, onPermissionCancelled } from '../../events/permissionEvents';
import { usePermissionStore } from '../../store/permissionStore';
import { selectAvatarArchetype } from '../../lib/agentIdentity';
import { AvatarOverlay, useAvatarState, useAvatarSpeech } from '../../components/avatar';
import { FEATURES } from '../../lib/features';

const PAGE_SIZE = 50;

interface QueuedInitialMessage {
  key: string;
  content: ContentBlock[];
}

interface ChatPanelProps {
  sessionId?: string;
  draft?: ChatDraft | null;
  onDraftTextChange?: (text: string) => void;
  onDraftSend?: (content: ContentBlock[]) => Promise<void>;
  initialQueuedMessage?: QueuedInitialMessage | null;
  onInitialMessageHandled?: (key: string) => void;
  onInitialMessageFailed?: (key: string) => void;
  agentIdentity?: AgentIdentityConfig;
}

let msgId = 0;

function finalizeStreamingMessage(message: DisplayMessage): DisplayMessage {
  const blocks = [...message.blocks];
  const lastBlock = blocks[blocks.length - 1];
  if (lastBlock && lastBlock.kind === 'text' && lastBlock.isStreaming) {
    blocks[blocks.length - 1] = { ...lastBlock, isStreaming: false };
  }
  return { ...message, blocks, isStreaming: false };
}

function contentBlocksToDisplay(content: ContentBlock[]): DisplayBlock[] {
  return content.map((block): DisplayBlock => {
    if (block.type === 'text') return { kind: 'text', text: block.text, isStreaming: false };
    if (block.type === 'image') {
      return { kind: 'image', mediaType: block.media_type, data: block.data };
    }
    return { kind: 'text', text: '[attachment]', isStreaming: false };
  });
}

export function ChatPanel({
  sessionId,
  draft,
  onDraftTextChange,
  onDraftSend,
  initialQueuedMessage,
  onInitialMessageHandled,
  onInitialMessageFailed,
  agentIdentity,
}: ChatPanelProps) {
  const queryClient = useQueryClient();
  const parentRef = useRef<HTMLDivElement>(null);
  const consumedInitialMessageRef = useRef<string | null>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const [streaming, setStreaming] = useState(false);
  const [streamMessages, setStreamMessages] = useState<DisplayMessage[]>([]);

  const prevScrollHeightRef = useRef(0);
  const prevScrollTopRef = useRef(0);
  const isLoadingOlderRef = useRef(false);
  const isDraft = Boolean(draft && !sessionId);
  const streamId = sessionId ? `chat:${sessionId}` : null;

  // ── Avatar ────────────────────────────────────────────────────────────────
  const avatarEnabled = FEATURES.avatar && (agentIdentity?.avatarEnabled ?? false);
  const [avatarVisible, setAvatarVisible] = useState(true);
  const [avatarSpeakAloud, setAvatarSpeakAloud] = useState(
    agentIdentity?.avatarSpeakAloud ?? false
  );
  const resolvedArchetype = useMemo(
    () => selectAvatarArchetype(agentIdentity ?? { presetId: 'balanced_assistant', identityName: 'Assistant', voice: 'neutral', vibe: '', warmth: 55, directness: 55, humor: 20, avatarEnabled: false, avatarArchetype: 'auto', avatarSpeakAloud: false }),
    [agentIdentity]
  );
  const { state: avatarState, forceThinking } = useAvatarState(isDraft ? null : streamId);
  useAvatarSpeech(avatarState, agentIdentity, avatarSpeakAloud);

  // Reset avatar visibility/speak when session changes
  useEffect(() => {
    setAvatarVisible(true);
    setAvatarSpeakAloud(agentIdentity?.avatarSpeakAloud ?? false);
  }, [sessionId, agentIdentity?.avatarSpeakAloud]);

  useEffect(() => {
    setStreaming(false);
    setStreamMessages([]);
    setAutoScroll(true);
    consumedInitialMessageRef.current = null;
  }, [draft?.id, sessionId]);

  const { data, fetchNextPage, hasNextPage, isFetchingNextPage } = useInfiniteQuery({
    queryKey: sessionId ? ['chat-messages', sessionId] : ['chat-messages', 'draft'],
    queryFn: async ({ pageParam = 0 }) => {
      return chatApi.getMessagesPaginated(sessionId!, PAGE_SIZE, pageParam);
    },
    initialPageParam: 0,
    getNextPageParam: (lastPage, allPages) => {
      if (!lastPage.hasMore) return undefined;
      const totalLoaded = allPages.reduce((sum, page) => sum + page.messages.length, 0);
      return totalLoaded;
    },
    refetchInterval: streaming ? false : 10_000,
    refetchOnWindowFocus: false,
    staleTime: 30_000,
    gcTime: 10 * 60_000,
    enabled: Boolean(sessionId),
  });

  const allDbMessages = useMemo(() => {
    if (!data?.pages) return [];
    const reversed = [...data.pages].reverse();
    const all = reversed.flatMap((page) => page.messages);
    const seen = new Set<string>();
    return all.filter((msg) => {
      const key = `${msg.created_at}:${msg.role}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
  }, [data]);

  const { data: reactionsData } = useQuery({
    queryKey: ['message-reactions', sessionId],
    queryFn: () => chatApi.getReactions(sessionId!),
    enabled: Boolean(sessionId),
    staleTime: 30_000,
  });

  const finalizeStreamingState = useCallback(() => {
    setStreaming(false);
    setStreamMessages((prev) => {
      const msgs = [...prev];
      const last = msgs[msgs.length - 1];
      if (last && last.isStreaming) {
        msgs[msgs.length - 1] = finalizeStreamingMessage(last);
      }
      return msgs;
    });
    if (sessionId) {
      queryClient.invalidateQueries({ queryKey: ['chat-messages', sessionId] });
      queryClient.invalidateQueries({ queryKey: ['chat-sessions'] });
      queryClient.invalidateQueries({ queryKey: ['chat-session-execution', sessionId] });
    }
  }, [queryClient, sessionId]);

  const { data: sessionExecution } = useQuery({
    queryKey: ['chat-session-execution', sessionId],
    queryFn: () => chatApi.getSessionExecution(sessionId!),
    enabled: Boolean(sessionId) && streaming,
    refetchInterval: streaming ? 5_000 : false,
    refetchOnWindowFocus: false,
    staleTime: 0,
  });

  const historyMessages = useMemo(() => {
    if (!sessionId) return [];
    const msgs = chatMessagesToDisplay(allDbMessages);
    if (reactionsData) {
      const byMsg = new Map<string, Array<{ id: string; emoji: string }>>();
      for (const r of reactionsData) {
        const arr = byMsg.get(r.messageId) ?? [];
        arr.push({ id: r.id, emoji: r.emoji });
        byMsg.set(r.messageId, arr);
      }
      for (const msg of msgs) {
        if (msg.dbId && byMsg.has(msg.dbId)) {
          msg.reactions = byMsg.get(msg.dbId);
        }
      }
    }
    return msgs;
  }, [allDbMessages, sessionId, reactionsData]);

  const optimisticInitialMessages = useMemo<DisplayMessage[]>(() => {
    if (isDraft || !initialQueuedMessage) return [];

    return [
      {
        id: `queued-user-${initialQueuedMessage.key}`,
        role: 'user',
        blocks: contentBlocksToDisplay(initialQueuedMessage.content),
        isStreaming: false,
        timestamp: new Date().toISOString(),
      },
      {
        id: `queued-assistant-${initialQueuedMessage.key}`,
        role: 'assistant',
        blocks: [],
        isStreaming: true,
      },
    ];
  }, [initialQueuedMessage, isDraft]);

  const shouldPreferStreamMessages = streaming || streamMessages.length > historyMessages.length;
  const displayMessages = isDraft
    ? []
    : shouldPreferStreamMessages
      ? streamMessages
      : historyMessages.length > 0
        ? historyMessages
        : optimisticInitialMessages;

  useEffect(() => {
    if (!streamId || !sessionId) return;

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
        // Hide react_to_message tool calls — no bubble
        if (payload.block.type === 'tool_use' && payload.block.name === 'react_to_message') return;
        setStreamMessages((prev) => {
          const msgs = [...prev];
          let last = msgs[msgs.length - 1];
          if (!last || last.role !== 'assistant') return prev;
          last = { ...last };
          msgs[msgs.length - 1] = last;

          const blocks = [...last.blocks];
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
        if (payload.action === 'llm_call' && payload.iteration > 1) {
          setStreamMessages((prev) => {
            const msgs = [...prev];
            const last = msgs[msgs.length - 1];
            if (last && last.role === 'assistant' && last.isStreaming) {
              msgs[msgs.length - 1] = finalizeStreamingMessage(last);
            }
            return msgs;
          });
        }
        if (payload.action === 'finished') {
          finalizeStreamingState();
        }
      })
    );

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

    unsubs.push(
      onPermissionCancelled((payload) => {
        usePermissionStore.getState().removeRequest(payload.requestId);
      })
    );

    unsubs.push(
      onUserQuestion((payload) => {
        if (payload.runId !== streamId && payload.sessionId !== sessionId) return;
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
              kind: 'user_question_prompt' as const,
              requestId: payload.requestId,
              question: payload.question,
              choices: payload.choices ?? undefined,
              allowCustom: payload.allowCustom,
              multiSelect: payload.multiSelect,
              context: payload.context ?? undefined,
            },
          ];
          return msgs;
        });
      })
    );

    unsubs.push(
      onMessageReaction((payload) => {
        if (payload.sessionId !== sessionId) return;
        // Apply reaction to stream messages with animation
        setStreamMessages((prev) => {
          const msgs = [...prev];
          for (let i = 0; i < msgs.length; i++) {
            if (msgs[i].dbId === payload.messageId) {
              const updated = { ...msgs[i] };
              updated.reactions = [
                ...(updated.reactions ?? []),
                { id: payload.reactionId, emoji: payload.emoji, isNew: true },
              ];
              msgs[i] = updated;
              return msgs;
            }
          }
          return prev;
        });
        // Invalidate the reactions query so history view stays in sync
        queryClient.invalidateQueries({ queryKey: ['message-reactions', sessionId] });
      })
    );

    return () => {
      unsubs.forEach((p) => p.then((unsub) => unsub()).catch(() => {}));
    };
  }, [finalizeStreamingState, queryClient, sessionId, streamId]);

  useEffect(() => {
    if (!streaming || !sessionExecution?.executionState) return;
    if (
      sessionExecution.executionState === 'queued' ||
      sessionExecution.executionState === 'running' ||
      sessionExecution.executionState === 'waiting_message' ||
      sessionExecution.executionState === 'waiting_user' ||
      sessionExecution.executionState === 'waiting_timeout' ||
      sessionExecution.executionState === 'waiting_sub_agents'
    ) {
      return;
    }
    finalizeStreamingState();
  }, [finalizeStreamingState, sessionExecution?.executionState, streaming]);

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

  useEffect(() => {
    if (autoScroll && displayMessages.length > 0 && !isLoadingOlderRef.current) {
      requestAnimationFrame(() => {
        virtualizer.scrollToIndex(displayMessages.length - 1, { align: 'end' });
      });
    }
  }, [autoScroll, displayMessages, virtualizer]);

  const handleLoadOlder = useCallback(() => {
    if (!parentRef.current || !hasNextPage || isFetchingNextPage) return;
    const el = parentRef.current;
    prevScrollHeightRef.current = el.scrollHeight;
    prevScrollTopRef.current = el.scrollTop;
    isLoadingOlderRef.current = true;
    void fetchNextPage();
  }, [fetchNextPage, hasNextPage, isFetchingNextPage]);

  function handleScroll() {
    if (!parentRef.current) return;
    const { scrollTop, scrollHeight, clientHeight } = parentRef.current;
    setAutoScroll(scrollHeight - scrollTop - clientHeight < 50);

    if (scrollTop < 200 && hasNextPage && !isFetchingNextPage) {
      handleLoadOlder();
    }
  }

  const handlePersistedSend = useCallback(
    async (content: ContentBlock[]) => {
      if (!sessionId) return;

      const userMsg: DisplayMessage = {
        id: `user-${++msgId}`,
        role: 'user',
        blocks: contentBlocksToDisplay(content),
        isStreaming: false,
        timestamp: new Date().toISOString(),
      };

      const assistantPlaceholder: DisplayMessage = {
        id: `assistant-${++msgId}`,
        role: 'assistant',
        blocks: [],
        isStreaming: true,
      };

      setStreamMessages([...historyMessages, userMsg, assistantPlaceholder]);
      setStreaming(true);
      forceThinking();

      try {
        const resp = await chatApi.sendMessage(sessionId, content);
        // Set the dbId so reactions can target this message
        setStreamMessages((prev) =>
          prev.map((m) =>
            m.id === userMsg.id ? { ...m, dbId: resp.userMessageId } : m
          )
        );
      } catch (err) {
        console.error('Failed to send message:', err);
        setStreaming(false);
        throw err;
      }
    },
    [historyMessages, sessionId]
  );

  const handleSend = useCallback(
    async (content: ContentBlock[]) => {
      if (isDraft) {
        if (!onDraftSend) return;
        await onDraftSend(content);
        return;
      }

      await handlePersistedSend(content);
    },
    [handlePersistedSend, isDraft, onDraftSend]
  );

  const queuedMessageKey = initialQueuedMessage?.key;
  const queuedMessageContent = initialQueuedMessage?.content;

  useEffect(() => {
    if (!sessionId || !queuedMessageKey || !queuedMessageContent) return;
    if (consumedInitialMessageRef.current === queuedMessageKey) return;

    consumedInitialMessageRef.current = queuedMessageKey;

    const run = async () => {
      try {
        await handlePersistedSend(queuedMessageContent);
        onInitialMessageHandled?.(queuedMessageKey);
      } catch {
        consumedInitialMessageRef.current = null;
        onInitialMessageFailed?.(queuedMessageKey);
      }
    };

    void run();
  }, [
    handlePersistedSend,
    queuedMessageKey,
    queuedMessageContent,
    onInitialMessageFailed,
    onInitialMessageHandled,
    sessionId,
  ]);

  const showScrollBtn = !isDraft && !autoScroll;

  return (
    <div className="flex flex-col h-full">
      <div className="relative flex-1 min-h-0">
        <div
          ref={parentRef}
          onScroll={handleScroll}
          className="h-full overflow-y-auto overflow-x-hidden"
        >
          {isFetchingNextPage && !isDraft && (
            <div className="flex items-center justify-center gap-2 py-3">
              <Loader2 size={14} className="animate-spin text-muted" />
              <span className="text-muted text-xs">Loading older messages...</span>
            </div>
          )}

          {displayMessages.length === 0 ? (
            <div className="flex h-full items-center justify-center px-6">
              <div className="max-w-sm text-center">
                {isDraft && (
                  <div className="mb-3 inline-flex items-center rounded-full border border-dashed border-accent/50 bg-accent/10 px-3 py-1 text-[10px] font-semibold uppercase tracking-[0.18em] text-accent-hover">
                    Draft Chat
                  </div>
                )}
                <div className="text-sm text-muted">Send a message to start the conversation.</div>
              </div>
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

        {avatarEnabled && (
          <AvatarOverlay
            archetype={resolvedArchetype}
            state={avatarState}
            visible={avatarVisible}
            speakAloud={avatarSpeakAloud}
            onToggleVisible={() => setAvatarVisible((v) => !v)}
            onToggleSpeakAloud={() => setAvatarSpeakAloud((v) => !v)}
          />
        )}
      </div>

      <ChatInput
        onSend={handleSend}
        disabled={streaming}
        textValue={isDraft ? (draft?.text ?? '') : undefined}
        onTextChange={isDraft ? onDraftTextChange : undefined}
        contextGauge={
          sessionId ? (
            <ContextGauge
              sessionId={sessionId}
              onCompacted={() => {
                queryClient.invalidateQueries({ queryKey: ['chat-messages', sessionId] });
              }}
            />
          ) : undefined
        }
      />
    </div>
  );
}
