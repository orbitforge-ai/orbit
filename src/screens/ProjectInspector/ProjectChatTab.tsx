import { useState, useEffect, useMemo } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Bot, ChevronDown, Copy, FolderOpen, Users } from 'lucide-react';
import * as Select from '@radix-ui/react-select';
import { chatApi } from '../../api/chat';
import { projectsApi } from '../../api/projects';
import { workspaceApi } from '../../api/workspace';
import { useUiStore } from '../../store/uiStore';
import {
  draftToChatSession,
  getDraftSessionId,
  isDraftSessionId,
  useChatDraftStore,
} from '../../store/chatDraftStore';
import { ContentBlock } from '../../types';
import { SessionList } from '../Chat/SessionList';
import { ChatPanel } from '../Chat/ChatPanel';

interface PendingInitialSend {
  key: string;
  sessionId: string;
  agentId: string;
  draftId: string;
  content: ContentBlock[];
}

export function ProjectChatTab({ projectId }: { projectId: string }) {
  const queryClient = useQueryClient();
  const { setProjectTab } = useUiStore();
  const drafts = useChatDraftStore((state) => state.drafts);
  const ensureDraft = useChatDraftStore((state) => state.ensureDraft);
  const updateDraftText = useChatDraftStore((state) => state.updateDraftText);
  const deleteDraft = useChatDraftStore((state) => state.deleteDraft);

  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [pendingInitialSend, setPendingInitialSend] = useState<PendingInitialSend | null>(null);
  const [copied, setCopied] = useState(false);

  const { data: projectAgents = [] } = useQuery({
    queryKey: ['project-agents-meta', projectId],
    queryFn: () => projectsApi.listAgentsWithMeta(projectId),
  });

  const { data: workspacePath } = useQuery({
    queryKey: ['project-workspace-path', projectId],
    queryFn: () => projectsApi.getWorkspacePath(projectId),
    staleTime: 60_000,
  });

  const { data: agentConfig } = useQuery({
    queryKey: ['agent-config', selectedAgentId],
    queryFn: () => workspaceApi.getConfig(selectedAgentId!),
    enabled: Boolean(selectedAgentId),
    staleTime: 60_000,
  });

  const draftScope = useMemo(
    () => (selectedAgentId ? { agentId: selectedAgentId, projectId } : null),
    [selectedAgentId, projectId]
  );
  const draftKey = draftScope ? `${projectId}:${draftScope.agentId}` : null;
  const selectedDraft = draftKey ? drafts[draftKey] ?? null : null;
  const visibleDraft =
    selectedAgentId && pendingInitialSend?.agentId === selectedAgentId ? null : selectedDraft;
  const draftSession = visibleDraft ? draftToChatSession(visibleDraft) : null;

  // Pick default agent (is_default first, fall back to first agent)
  useEffect(() => {
    if (selectedAgentId) {
      const stillMember = projectAgents.some((entry) => entry.agent.id === selectedAgentId);
      if (!stillMember) {
        setSelectedAgentId(null);
        setActiveSessionId(null);
      }
    }
    if (!selectedAgentId && projectAgents.length > 0) {
      const defaultAgent = projectAgents.find((entry) => entry.isDefault) ?? projectAgents[0];
      setSelectedAgentId(defaultAgent.agent.id);
    }
  }, [projectAgents, selectedAgentId]);

  useEffect(() => {
    if (!draftScope) return;

    if (isDraftSessionId(activeSessionId) && !selectedDraft) {
      setActiveSessionId(null);
      return;
    }

    if (!activeSessionId && selectedDraft) {
      setActiveSessionId(getDraftSessionId(draftScope));
    }
  }, [activeSessionId, draftScope, selectedDraft]);

  function handleSelectAgent(agentId: string) {
    setSelectedAgentId(agentId);
    const scope = { agentId, projectId };
    const key = `${projectId}:${agentId}`;
    setActiveSessionId(drafts[key] ? getDraftSessionId(scope) : null);
  }

  function handleNewSession() {
    if (!draftScope) return;
    ensureDraft(draftScope);
    setActiveSessionId(getDraftSessionId(draftScope));
  }

  function handleDeleteDraft() {
    if (!draftScope) return;
    deleteDraft(draftScope);
    if (activeSessionId === getDraftSessionId(draftScope)) {
      setActiveSessionId(null);
    }
  }

  async function handleDraftSend(content: ContentBlock[]) {
    if (!selectedAgentId || !draftScope || !draftKey) return;
    const draft = useChatDraftStore.getState().drafts[draftKey] ?? ensureDraft(draftScope);
    const session = await chatApi.createSession(selectedAgentId, undefined, undefined, projectId);
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
      const scope = { agentId: current.agentId, projectId };
      const draft = useChatDraftStore.getState().drafts[`${projectId}:${current.agentId}`];
      if (draft?.id === current.draftId) {
        useChatDraftStore.getState().deleteDraft(scope);
      }
      return null;
    });
  }

  function handleInitialMessageFailed(key: string) {
    setPendingInitialSend((current) => {
      if (!current || current.key !== key) return current;
      if (selectedAgentId === current.agentId) {
        setActiveSessionId(getDraftSessionId({ agentId: current.agentId, projectId }));
      }
      return null;
    });
  }

  async function copyWorkspacePath() {
    if (!workspacePath) return;
    try {
      await navigator.clipboard.writeText(workspacePath);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch (err) {
      console.warn('Failed to copy workspace path:', err);
    }
  }

  if (projectAgents.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-4 p-8 text-muted">
        <Users size={36} className="opacity-30" />
        <p className="text-sm font-medium">No agents assigned to this project</p>
        <p className="text-xs text-center max-w-sm">
          Assign at least one agent to this project to start chatting in its workspace.
        </p>
        <button
          onClick={() => setProjectTab('agents')}
          className="px-4 py-2 rounded-lg bg-accent hover:bg-accent-hover text-white text-sm font-medium transition-colors"
        >
          Go to Agents
        </button>
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      {workspacePath && (
        <div className="flex items-center gap-2 px-4 py-2 border-b border-edge bg-surface/30 text-[11px] text-muted">
          <FolderOpen size={12} className="text-accent-hover" />
          <span className="truncate font-mono">{workspacePath}</span>
          <button
            onClick={copyWorkspacePath}
            className="ml-auto flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] text-muted hover:text-white hover:bg-surface-hover transition-colors"
            title="Copy workspace path"
          >
            <Copy size={10} />
            {copied ? 'Copied' : 'Copy'}
          </button>
        </div>
      )}

      <div className="flex flex-1 min-h-0">
        <div className="w-[280px] flex flex-col border-r border-edge bg-panel">
          <div className="px-4 py-3 border-b border-edge">
            <label className="text-[10px] uppercase tracking-wider text-muted mb-1.5 block">
              Agent
            </label>
            {selectedAgentId && (
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
                      {projectAgents.map(({ agent, isDefault }) => (
                        <Select.Item
                          key={agent.id}
                          value={agent.id}
                          className="flex items-center gap-2 px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-accent/20"
                        >
                          <Bot size={12} className="text-accent-hover" />
                          <Select.ItemText>{agent.name}</Select.ItemText>
                          {isDefault && (
                            <span className="ml-auto rounded-full border border-accent/40 bg-accent/12 px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-[0.16em] text-accent-hover">
                              Default
                            </span>
                          )}
                        </Select.Item>
                      ))}
                    </Select.Viewport>
                  </Select.Content>
                </Select.Portal>
              </Select.Root>
            )}
          </div>

          {selectedAgentId && (
            <SessionList
              agentId={selectedAgentId}
              activeSessionId={activeSessionId}
              onSelectSession={setActiveSessionId}
              onNewSession={handleNewSession}
              draftSession={draftSession}
              onDeleteDraft={handleDeleteDraft}
              projectId={projectId}
            />
          )}
        </div>

        <div className="flex-1 min-h-0">
          {selectedDraft &&
          draftScope &&
          activeSessionId === getDraftSessionId(draftScope) ? (
            <ChatPanel
              draft={selectedDraft}
              onDraftTextChange={(text) => updateDraftText(draftScope, text)}
              onDraftSend={handleDraftSend}
              agentIdentity={agentConfig?.identity}
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
              agentIdentity={agentConfig?.identity}
            />
          ) : (
            <div className="flex items-center justify-center h-full text-muted text-sm">
              Select or create a chat session
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
