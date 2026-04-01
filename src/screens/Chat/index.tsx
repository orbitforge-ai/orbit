import { useState, useEffect } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Bot, ChevronDown } from 'lucide-react';
import * as Select from '@radix-ui/react-select';
import { agentsApi } from '../../api/agents';
import { chatApi } from '../../api/chat';
import { useUiStore } from '../../store/uiStore';
import {
  draftToChatSession,
  getDraftSessionId,
  isDraftSessionId,
  useChatDraftStore,
} from '../../store/chatDraftStore';
import { ChatSession, ContentBlock } from '../../types';
import { SessionList } from './SessionList';
import { ChatPanel } from './ChatPanel';

interface PendingInitialSend {
  key: string;
  sessionId: string;
  agentId: string;
  draftId: string;
  content: ContentBlock[];
}

export function ChatScreen() {
  const queryClient = useQueryClient();
  const { pendingChatSessionId, clearPendingChatSession } = useUiStore();
  const drafts = useChatDraftStore((state) => state.drafts);
  const ensureDraft = useChatDraftStore((state) => state.ensureDraft);
  const updateDraftText = useChatDraftStore((state) => state.updateDraftText);
  const deleteDraft = useChatDraftStore((state) => state.deleteDraft);
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [pendingInitialSend, setPendingInitialSend] = useState<PendingInitialSend | null>(null);

  const { data: agents = [] } = useQuery({
    queryKey: ['agents'],
    queryFn: agentsApi.list,
  });

  const selectedDraft = selectedAgentId ? drafts[selectedAgentId] ?? null : null;
  const visibleDraft =
    selectedAgentId && pendingInitialSend?.agentId === selectedAgentId ? null : selectedDraft;
  const draftSession = visibleDraft ? draftToChatSession(visibleDraft) : null;

  useEffect(() => {
    if (!pendingChatSessionId || agents.length === 0) return;

    async function resolve() {
      for (const agent of agents) {
        const sessions: ChatSession[] = await chatApi.listSessions(agent.id, true);
        const match = sessions.find((session) => session.id === pendingChatSessionId);
        if (match) {
          setSelectedAgentId(agent.id);
          setActiveSessionId(match.id);
          clearPendingChatSession();
          return;
        }
      }
      clearPendingChatSession();
    }

    void resolve();
  }, [agents, clearPendingChatSession, pendingChatSessionId]);

  useEffect(() => {
    if (!selectedAgentId && agents.length > 0) {
      setSelectedAgentId(agents[0].id);
    }
  }, [agents, selectedAgentId]);

  useEffect(() => {
    if (!selectedAgentId) return;

    if (isDraftSessionId(activeSessionId) && !selectedDraft) {
      setActiveSessionId(null);
      return;
    }

    if (!activeSessionId && selectedDraft) {
      setActiveSessionId(getDraftSessionId(selectedAgentId));
    }
  }, [activeSessionId, selectedAgentId, selectedDraft]);

  function handleSelectAgent(agentId: string) {
    setSelectedAgentId(agentId);
    setActiveSessionId(drafts[agentId] ? getDraftSessionId(agentId) : null);
  }

  function handleNewSession() {
    if (!selectedAgentId) return;
    ensureDraft(selectedAgentId);
    setActiveSessionId(getDraftSessionId(selectedAgentId));
  }

  function handleDeleteDraft() {
    if (!selectedAgentId) return;
    deleteDraft(selectedAgentId);
    if (activeSessionId === getDraftSessionId(selectedAgentId)) {
      setActiveSessionId(null);
    }
  }

  async function handleDraftSend(content: ContentBlock[]) {
    if (!selectedAgentId) return;

    const draft = useChatDraftStore.getState().drafts[selectedAgentId] ?? ensureDraft(selectedAgentId);
    const session = await chatApi.createSession(selectedAgentId);
    queryClient.invalidateQueries({ queryKey: ['chat-sessions'] });
    setPendingInitialSend({
      key: `${session.id}:${Date.now()}`,
      sessionId: session.id,
      agentId: selectedAgentId,
      draftId: draft.id,
      content,
    });
    setActiveSessionId(session.id);
  }

  function handleInitialMessageHandled(key: string) {
    setPendingInitialSend((current) => {
      if (!current || current.key !== key) return current;
      const draft = useChatDraftStore.getState().drafts[current.agentId];
      if (draft?.id === current.draftId) {
        useChatDraftStore.getState().deleteDraft(current.agentId);
      }
      return null;
    });
  }

  function handleInitialMessageFailed(key: string) {
    setPendingInitialSend((current) => {
      if (!current || current.key !== key) return current;
      if (selectedAgentId === current.agentId) {
        setActiveSessionId(getDraftSessionId(current.agentId));
      }
      return null;
    });
  }

  return (
    <div className="flex h-full">
      <div className="w-[280px] flex flex-col border-r border-edge bg-panel">
        <div className="px-4 py-3 border-b border-edge">
          <label className="text-[10px] uppercase tracking-wider text-muted mb-1.5 block">
            Agent
          </label>
          {agents.length > 0 && selectedAgentId && (
            <Select.Root value={selectedAgentId} onValueChange={handleSelectAgent}>
              <Select.Trigger className="flex items-center justify-between w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent">
                <div className="flex items-center gap-2">
                  <Bot size={14} className="text-accent-hover" />
                  <Select.Value />
                </div>
                <Select.Icon>
                  <ChevronDown size={14} className="text-muted" />
                </Select.Icon>
              </Select.Trigger>
              <Select.Portal>
                <Select.Content className="rounded-lg bg-surface border border-edge shadow-xl overflow-hidden z-50">
                  <Select.Viewport className="p-1">
                    {agents.map((agent) => (
                      <Select.Item
                        key={agent.id}
                        value={agent.id}
                        className="flex items-center gap-2 px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-accent/20"
                      >
                        <Bot size={12} className="text-accent-hover" />
                        <Select.ItemText>{agent.name}</Select.ItemText>
                      </Select.Item>
                    ))}
                  </Select.Viewport>
                </Select.Content>
              </Select.Portal>
            </Select.Root>
          )}
        </div>

        {selectedAgentId ? (
          <SessionList
            agentId={selectedAgentId}
            activeSessionId={activeSessionId}
            onSelectSession={setActiveSessionId}
            onNewSession={handleNewSession}
            draftSession={draftSession}
            onDeleteDraft={handleDeleteDraft}
          />
        ) : (
          <div className="flex-1 flex items-center justify-center text-muted text-xs">
            Select an agent to start chatting
          </div>
        )}
      </div>

      <div className="flex-1 min-h-0">
        {selectedDraft &&
        selectedAgentId &&
        activeSessionId === getDraftSessionId(selectedAgentId) ? (
          <ChatPanel
            draft={selectedDraft}
            onDraftTextChange={(text) => updateDraftText(selectedAgentId, text)}
            onDraftSend={handleDraftSend}
          />
        ) : activeSessionId && !isDraftSessionId(activeSessionId) ? (
          <ChatPanel
            sessionId={activeSessionId}
            initialQueuedMessage={
              pendingInitialSend?.sessionId === activeSessionId
                ? { key: pendingInitialSend.key, content: pendingInitialSend.content }
                : null
            }
            onInitialMessageHandled={handleInitialMessageHandled}
            onInitialMessageFailed={handleInitialMessageFailed}
          />
        ) : (
          <div className="flex items-center justify-center h-full text-muted text-sm">
            Select or create a chat session
          </div>
        )}
      </div>
    </div>
  );
}
