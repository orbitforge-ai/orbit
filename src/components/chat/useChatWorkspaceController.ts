import { useEffect, useMemo, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { chatApi } from '../../api/chat';
import {
  draftToChatSession,
  getDraftScopeKey,
  getDraftSessionId,
  isDraftSessionId,
  useChatDraftStore,
} from '../../store/chatDraftStore';
import { ChatDraft, ChatModelOverride, ChatSession, ContentBlock } from '../../types';

type ChatWorkspaceSelectionMode = 'empty' | 'latest-user-chat';

interface QueuedInitialMessage {
  key: string;
  sessionId: string;
  content: ContentBlock[];
  modelOverride: ChatModelOverride | null;
}

interface PendingInitialSend extends QueuedInitialMessage {
  agentId: string;
  draftId: string;
  projectId: string | null;
}

export interface UseChatWorkspaceControllerOptions {
  agentId: string | null;
  projectId?: string | null;
  pendingSessionId?: string | null;
  selectionMode?: ChatWorkspaceSelectionMode;
  onPendingSessionHandled?: () => void;
}

export interface UseChatWorkspaceControllerResult {
  activeSessionId: string | null;
  setActiveSessionId: (sessionId: string | null) => void;
  draft: ChatDraft | null;
  draftSession: ChatSession | null;
  handleNewSession: () => void;
  handleDeleteDraft: () => void;
  handleDraftTextChange: (text: string) => void;
  handleDraftSend: (content: ContentBlock[], modelOverride?: ChatModelOverride | null) => Promise<void>;
  initialQueuedMessage: QueuedInitialMessage | null;
  handleInitialMessageHandled: (key: string) => void;
  handleInitialMessageFailed: (key: string) => void;
}

function getLatestUserChat(sessions: ChatSession[]) {
  return [...sessions]
    .filter((session) => session.sessionType === 'user_chat')
    .sort((a, b) => new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime())[0];
}

export function useChatWorkspaceController({
  agentId,
  projectId = null,
  pendingSessionId = null,
  selectionMode = 'empty',
  onPendingSessionHandled,
}: UseChatWorkspaceControllerOptions): UseChatWorkspaceControllerResult {
  const queryClient = useQueryClient();
  const drafts = useChatDraftStore((state) => state.drafts);
  const ensureDraft = useChatDraftStore((state) => state.ensureDraft);
  const updateDraftText = useChatDraftStore((state) => state.updateDraftText);
  const deleteDraft = useChatDraftStore((state) => state.deleteDraft);

  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [pendingInitialSend, setPendingInitialSend] = useState<PendingInitialSend | null>(null);

  const skipAutoSelectRef = useRef(false);
  const handledPendingSessionIdRef = useRef<string | null>(null);

  const draftScope = useMemo(() => {
    if (!agentId) return null;
    return {
      agentId,
      projectId: projectId ?? undefined,
    };
  }, [agentId, projectId]);

  const draftScopeKey = useMemo(
    () => (draftScope ? getDraftScopeKey(draftScope) : null),
    [draftScope]
  );

  const selectedDraft = draftScopeKey ? drafts[draftScopeKey] ?? null : null;
  const visibleDraft = agentId && pendingInitialSend?.agentId === agentId ? null : selectedDraft;
  const draftSession = visibleDraft ? draftToChatSession(visibleDraft) : null;

  const { data: chatSessions = [], isFetched: chatSessionsFetched } = useQuery({
    queryKey: ['chat-sessions', agentId, false, projectId ?? null],
    queryFn: () => chatApi.listSessions(agentId!, false, undefined, projectId ?? undefined),
    enabled: Boolean(agentId),
  });

  useEffect(() => {
    setActiveSessionId(null);
    setPendingInitialSend(null);
    skipAutoSelectRef.current = false;
    handledPendingSessionIdRef.current = null;
  }, [agentId, projectId]);

  useEffect(() => {
    if (!pendingSessionId) {
      handledPendingSessionIdRef.current = null;
    }
  }, [pendingSessionId]);

  useEffect(() => {
    if (!draftScope || !chatSessionsFetched) return;

    if (
      pendingSessionId &&
      handledPendingSessionIdRef.current !== pendingSessionId
    ) {
      handledPendingSessionIdRef.current = pendingSessionId;
      const pendingSession = chatSessions.find((session) => session.id === pendingSessionId);
      onPendingSessionHandled?.();
      if (pendingSession) {
        setActiveSessionId(pendingSession.id);
        return;
      }
    }

    if (pendingInitialSend?.agentId === draftScope.agentId) {
      if (activeSessionId !== pendingInitialSend.sessionId) {
        setActiveSessionId(pendingInitialSend.sessionId);
      }
      return;
    }

    if (activeSessionId && chatSessions.some((session) => session.id === activeSessionId)) {
      return;
    }

    if (activeSessionId && isDraftSessionId(activeSessionId) && selectedDraft) {
      return;
    }

    if (!activeSessionId && skipAutoSelectRef.current) {
      skipAutoSelectRef.current = false;
      return;
    }

    if (selectedDraft) {
      setActiveSessionId(getDraftSessionId(draftScope));
      return;
    }

    if (activeSessionId) {
      setActiveSessionId(null);
      return;
    }

    if (selectionMode === 'latest-user-chat') {
      const latestUserChat = getLatestUserChat(chatSessions);
      if (latestUserChat) {
        setActiveSessionId(latestUserChat.id);
      }
    }
  }, [
    activeSessionId,
    chatSessions,
    chatSessionsFetched,
    draftScope,
    onPendingSessionHandled,
    pendingInitialSend,
    pendingSessionId,
    selectedDraft,
    selectionMode,
  ]);

  function handleNewSession() {
    if (!draftScope) return;
    ensureDraft(draftScope);
    setActiveSessionId(getDraftSessionId(draftScope));
  }

  function handleDeleteDraft() {
    if (!draftScope) return;
    deleteDraft(draftScope);
    if (activeSessionId === getDraftSessionId(draftScope)) {
      skipAutoSelectRef.current = true;
      setActiveSessionId(null);
    }
  }

  function handleDraftTextChange(text: string) {
    if (!draftScope) return;
    updateDraftText(draftScope, text);
  }

  async function handleDraftSend(content: ContentBlock[], modelOverride?: ChatModelOverride | null) {
    if (!agentId || !draftScope || !draftScopeKey) return;

    const draft = useChatDraftStore.getState().drafts[draftScopeKey] ?? ensureDraft(draftScope);
    const session = await chatApi.createSession(agentId, undefined, undefined, projectId ?? undefined);
    queryClient.invalidateQueries({ queryKey: ['chat-sessions'] });
    setPendingInitialSend({
      key: `${session.id}:${Date.now()}`,
      sessionId: session.id,
      agentId,
      draftId: draft.id,
      projectId,
      content,
      modelOverride: modelOverride ?? null,
    });
    setActiveSessionId(session.id);
  }

  function handleInitialMessageHandled(key: string) {
    setPendingInitialSend((current) => {
      if (!current || current.key !== key) return current;
      const scope = {
        agentId: current.agentId,
        projectId: current.projectId ?? undefined,
      };
      const draft = useChatDraftStore.getState().drafts[getDraftScopeKey(scope)];
      if (draft?.id === current.draftId) {
        useChatDraftStore.getState().deleteDraft(scope);
      }
      return null;
    });
  }

  function handleInitialMessageFailed(key: string) {
    setPendingInitialSend((current) => {
      if (!current || current.key !== key) return current;
      const scope = {
        agentId: current.agentId,
        projectId: current.projectId ?? undefined,
      };
      if (agentId === current.agentId && projectId === current.projectId) {
        setActiveSessionId(getDraftSessionId(scope));
      }
      return null;
    });
  }

  return {
    activeSessionId,
    setActiveSessionId,
    draft: visibleDraft,
    draftSession,
    handleNewSession,
    handleDeleteDraft,
    handleDraftTextChange,
    handleDraftSend,
    initialQueuedMessage: pendingInitialSend
      ? {
          key: pendingInitialSend.key,
          sessionId: pendingInitialSend.sessionId,
          content: pendingInitialSend.content,
          modelOverride: pendingInitialSend.modelOverride,
        }
      : null,
    handleInitialMessageHandled,
    handleInitialMessageFailed,
  };
}
