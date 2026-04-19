import { useEffect, useRef, useState, useMemo, useCallback } from 'react';
import { useInfiniteQuery, useQuery, useQueryClient } from '@tanstack/react-query';
import { useVirtualizer } from '@tanstack/react-virtual';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import { ArrowDown, Box, Check, ChevronDown, FolderOpen, Loader2, Shield } from 'lucide-react';
import { chatApi } from '../../api/chat';
import { workspaceApi } from '../../api/workspace';
import { useUiStore } from '../../store/uiStore';
import { MODEL_OPTIONS, LLM_PROVIDERS } from '../../constants/providers';
import { AgentIdentityConfig, ChatDraft, ChatModelOverride, ContentBlock } from '../../types';
import { DisplayMessage, DisplayBlock } from '../../components/chat/types';
import { chatMessagesToDisplay } from '../../components/chat/utils';
import { MessageBubble } from '../../components/chat/MessageBubble';
import { PermissionPrompt } from '../../components/chat/PermissionPrompt';
import { ChatInput } from './ChatInput';
import { ContextGauge } from '../../components/chat/ContextGauge';
import { onAgentConfigChanged } from '../../events/agentEvents';
import { useLiveChatStore } from '../../store/liveChatStore';
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
  const [selectedModelOverride, setSelectedModelOverride] = useState<ChatModelOverride | null>(
    null
  );
  const [modelPinned, setModelPinned] = useState(false);

  const prevScrollHeightRef = useRef(0);
  const prevScrollTopRef = useRef(0);
  const isLoadingOlderRef = useRef(false);
  const isDraft = Boolean(draft && !sessionId);
  const streamId = sessionId ? `chat:${sessionId}` : null;
  const streamEntry = useLiveChatStore(
    useCallback(
      (state) => (streamId ? state.chatStreams[streamId] ?? null : null),
      [streamId]
    )
  );
  const streaming = streamEntry?.isStreaming ?? false;
  const streamMessages = streamEntry?.displayMessages ?? [];
  const pendingPermissionRequestMap = usePermissionStore((s) => s.pending);

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
    setAutoScroll(true);
    consumedInitialMessageRef.current = null;
    setSelectedModelOverride(null);
    setModelPinned(false);
  }, [draft?.id, sessionId]);

  const { data: sessionMeta } = useQuery({
    queryKey: ['chat-session-meta', sessionId],
    queryFn: () => chatApi.getSessionMeta(sessionId!),
    enabled: Boolean(sessionId),
    staleTime: 60_000,
  });

  const { data: agentConfig } = useQuery({
    queryKey: ['agent-config', sessionMeta?.agentId],
    queryFn: () => workspaceApi.getConfig(sessionMeta!.agentId),
    enabled: Boolean(sessionMeta?.agentId),
    staleTime: 60_000,
  });

  useEffect(() => {
    if (!sessionMeta?.agentId) return;

    const unsub = onAgentConfigChanged((payload) => {
      if (payload.agentId !== sessionMeta.agentId) return;
      queryClient.invalidateQueries({ queryKey: ['agent-config', sessionMeta.agentId] });
    });

    return () => {
      unsub.then((fn) => fn()).catch(() => {});
    };
  }, [queryClient, sessionMeta?.agentId]);

  useEffect(() => {
    if (!agentConfig) return;
    if (modelPinned) return;
    setSelectedModelOverride((current) => {
      if (
        current?.provider === agentConfig.provider &&
        current?.model === agentConfig.model
      ) {
        return current;
      }
      return {
        provider: agentConfig.provider,
        model: agentConfig.model,
      };
    });
  }, [agentConfig, modelPinned]);

  const selectProject = useUiStore((state) => state.selectProject);
  const setProjectTab = useUiStore((state) => state.setProjectTab);

  function handleProjectBadgeClick() {
    if (!sessionMeta?.projectId) return;
    selectProject(sessionMeta.projectId);
    setProjectTab('chat');
  }

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
        blocks: initialQueuedMessage.content.map((block): DisplayBlock => {
          if (block.type === 'text') return { kind: 'text', text: block.text, isStreaming: false };
          if (block.type === 'image') {
            return { kind: 'image', mediaType: block.media_type, data: block.data };
          }
          return { kind: 'text', text: '[attachment]', isStreaming: false };
        }),
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

  const shouldPreferStreamMessages =
    streamMessages.length > 0 && (streaming || streamMessages.length > historyMessages.length);
  const displayMessages = isDraft
    ? []
    : shouldPreferStreamMessages
      ? streamMessages
      : historyMessages.length > 0
        ? historyMessages
        : optimisticInitialMessages;

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
    if (streamId) {
      useLiveChatStore.getState().completeChatStream(streamId);
      queryClient.invalidateQueries({ queryKey: ['chat-messages', sessionId] });
      queryClient.invalidateQueries({ queryKey: ['chat-sessions'] });
      queryClient.invalidateQueries({ queryKey: ['chat-session-execution', sessionId] });
    }
  }, [queryClient, sessionExecution?.executionState, sessionId, streamId, streaming]);

  useEffect(() => {
    if (!streamId || !streamEntry || streaming) return;
    if (historyMessages.length >= streamMessages.length && streamMessages.length > 0) {
      useLiveChatStore.getState().clearChatStream(streamId);
    }
  }, [historyMessages.length, streamEntry, streamId, streamMessages.length, streaming]);

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
      const currentStreamId = `chat:${sessionId}`;
      const localUserMessageId = useLiveChatStore
        .getState()
        .startChatStream(currentStreamId, sessionId, historyMessages, content);
      forceThinking();

      try {
        const resp = await chatApi.sendMessage(
          sessionId,
          content,
          selectedModelOverride ?? undefined
        );
        useLiveChatStore
          .getState()
          .setUserMessageDbId(currentStreamId, localUserMessageId, resp.userMessageId);
      } catch (err) {
        console.error('Failed to send message:', err);
        useLiveChatStore.getState().clearChatStream(currentStreamId);
        throw err;
      }
    },
    [historyMessages, selectedModelOverride, sessionId]
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
  const visiblePermissionRequestIds = useMemo(() => {
    const ids = new Set<string>();
    for (const message of displayMessages) {
      for (const block of message.blocks) {
        if (block.kind === 'permission_prompt') {
          ids.add(block.requestId);
        }
      }
    }
    return ids;
  }, [displayMessages]);
  const sessionPendingPermissionRequests = useMemo(() => {
    if (!sessionId && !streamId) return [];

    return Object.values(pendingPermissionRequestMap)
      .filter(
        (request) =>
          (request.sessionId === sessionId || request.runId === streamId) &&
          !visiblePermissionRequestIds.has(request.requestId)
      )
      .sort((a, b) => new Date(a.timestamp).getTime() - new Date(b.timestamp).getTime());
  }, [pendingPermissionRequestMap, sessionId, streamId, visiblePermissionRequestIds]);

  const modelPicker = useMemo(() => {
    if (!sessionId || !selectedModelOverride) return null;

    const currentProvider = LLM_PROVIDERS.find((provider) => provider.value === selectedModelOverride.provider);
    const currentOption = (MODEL_OPTIONS[selectedModelOverride.provider] ?? []).find(
      (option) => option.value === selectedModelOverride.model
    );
    const currentLabel = currentOption?.label ?? selectedModelOverride.model;
    const triggerTitle = currentProvider
      ? `${currentProvider.label} • ${currentLabel}`
      : currentLabel;

    return (
      <DropdownMenu.Root>
        <DropdownMenu.Trigger asChild>
          <button
            type="button"
            disabled={streaming}
            aria-label="Choose reply model"
            title={triggerTitle}
            className={`inline-flex items-center gap-1.5 rounded-lg border px-2.5 py-2 text-xs transition-[border-color,background-color,color] disabled:opacity-50 ${
              modelPinned
                ? 'border-accent/40 bg-accent/10 text-accent-hover'
                : 'border-edge bg-background text-muted hover:border-edge-hover hover:text-white'
            }`}
          >
            <Box size={14} />
            <ChevronDown size={12} className="opacity-70" />
          </button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content
            align="end"
            side="top"
            sideOffset={8}
            className="z-50 w-72 rounded-xl border border-edge bg-surface p-1.5 shadow-xl"
          >
            <div className="px-2.5 py-2 border-b border-edge/70">
              <p className="text-[10px] font-semibold uppercase tracking-[0.16em] text-muted">
                Reply Model
              </p>
              <p className="mt-1 text-xs text-secondary">{triggerTitle}</p>
            </div>
            <div className="max-h-72 overflow-y-auto py-1">
              {LLM_PROVIDERS.map((provider) => (
                <div key={provider.value}>
                  <p className="px-2.5 py-1 text-[10px] font-semibold uppercase tracking-[0.16em] text-muted">
                    {provider.label}
                  </p>
                  {(MODEL_OPTIONS[provider.value] ?? []).map((option) => {
                    const active =
                      selectedModelOverride.provider === provider.value &&
                      selectedModelOverride.model === option.value;

                    return (
                      <DropdownMenu.Item
                        key={`${provider.value}::${option.value}`}
                        onSelect={() => {
                          setSelectedModelOverride({
                            provider: provider.value,
                            model: option.value,
                          });
                          setModelPinned(true);
                        }}
                        className="flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm outline-none cursor-pointer hover:bg-accent/10 data-[highlighted]:bg-accent/10"
                      >
                        <Box
                          size={14}
                          className={active ? 'text-accent-light' : 'text-muted'}
                        />
                        <span className={`flex-1 ${active ? 'text-accent-light font-medium' : 'text-white'}`}>
                          {option.label}
                        </span>
                        {active && <Check size={12} className="text-accent-light" />}
                      </DropdownMenu.Item>
                    );
                  })}
                </div>
              ))}
            </div>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    );
  }, [modelPinned, selectedModelOverride, sessionId, streaming]);

  return (
    <div className="flex flex-col h-full">
      {sessionMeta?.projectId && sessionMeta?.projectName && (
        <button
          type="button"
          onClick={handleProjectBadgeClick}
          className="flex items-center gap-1.5 self-start ml-3 mt-2 rounded-full border border-accent/40 bg-accent/10 px-2.5 py-1 text-[10px] font-medium uppercase tracking-[0.14em] text-accent-hover hover:bg-accent/20 transition-colors"
          title="Open project"
        >
          <FolderOpen size={11} />
          in {sessionMeta.projectName}
        </button>
      )}
      <div className="relative flex-1 min-h-0">
        <div
          ref={parentRef}
          onScroll={handleScroll}
          className="h-full overflow-y-auto overflow-x-hidden"
        >
          {sessionPendingPermissionRequests.length > 0 && (
            <div className="border-b border-amber-500/15 bg-amber-500/5 px-4 py-3">
              <div className="mb-2 flex items-center gap-2 text-[10px] font-semibold uppercase tracking-[0.16em] text-amber-300">
                <Shield size={12} />
                Pending Approval
                {sessionPendingPermissionRequests.length > 1 ? 's' : ''}
              </div>
              <div className="space-y-2">
                {sessionPendingPermissionRequests.map((request) => (
                  <PermissionPrompt
                    key={request.requestId}
                    requestId={request.requestId}
                    toolName={request.toolName}
                    toolInput={request.toolInput}
                    riskLevel={request.riskLevel}
                    riskDescription={request.riskDescription}
                    suggestedPattern={request.suggestedPattern}
                    agentId={request.agentId}
                  />
                ))}
              </div>
            </div>
          )}

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
        modelPicker={modelPicker}
        textValue={isDraft ? (draft?.text ?? '') : undefined}
        onTextChange={isDraft ? onDraftTextChange : undefined}
        contextGauge={
          sessionId ? (
            <ContextGauge
              sessionId={sessionId}
              agentId={sessionMeta?.agentId}
              modelOverride={selectedModelOverride ?? undefined}
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
