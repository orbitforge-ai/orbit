import { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { workspaceApi } from '../../api/workspace';
import { isDraftSessionId } from '../../store/chatDraftStore';
import { SessionList } from '../../screens/Chat/SessionList';
import { ChatPanel } from '../../screens/Chat/ChatPanel';
import { UseChatWorkspaceControllerResult } from './useChatWorkspaceController';

export interface ChatWorkspaceProps {
  agentId: string;
  projectId?: string | null;
  controller: UseChatWorkspaceControllerResult;
  sidebarHeader?: ReactNode;
  sidebarWidth?: number;
  sidebarCollapsible?: boolean;
  sessionsCollapsed?: boolean;
  onToggleSessions?: () => void;
  emptyStateCopy?: string;
}

export function ChatWorkspace({
  agentId,
  projectId = null,
  controller,
  sidebarHeader,
  sidebarWidth = 280,
  sidebarCollapsible = false,
  sessionsCollapsed = false,
  onToggleSessions,
  emptyStateCopy = 'Select or create a chat session',
}: ChatWorkspaceProps) {
  const { data: agentConfig } = useQuery({
    queryKey: ['agent-config', agentId],
    queryFn: () => workspaceApi.getConfig(agentId),
    staleTime: 60_000,
  });

  const showSidebar = !sidebarCollapsible || !sessionsCollapsed;
  const showCollapseToggle = sidebarCollapsible && onToggleSessions;
  const activeQueuedMessage =
    controller.initialQueuedMessage?.sessionId === controller.activeSessionId
      ? {
          key: controller.initialQueuedMessage.key,
          content: controller.initialQueuedMessage.content,
        }
      : null;

  return (
    <div className="relative flex h-full w-full min-w-0 flex-1">
      {showCollapseToggle && (
        <button
          type="button"
          onClick={onToggleSessions}
          style={{ left: sessionsCollapsed ? 12 : sidebarWidth - 12 }}
          className="absolute top-1/2 z-10 -translate-x-1/2 -translate-y-1/2 rounded-full border border-edge bg-background p-1.5 text-muted shadow-sm transition-colors hover:border-edge-hover hover:text-white"
          title={sessionsCollapsed ? 'Show sessions' : 'Collapse sessions'}
          aria-label={sessionsCollapsed ? 'Show sessions' : 'Collapse sessions'}
        >
          {sessionsCollapsed ? <ChevronRight size={14} /> : <ChevronLeft size={14} />}
        </button>
      )}

      {showSidebar && (
        <div style={{ width: sidebarWidth }} className="flex-shrink-0 border-r border-edge bg-panel">
          {sidebarHeader}
          <SessionList
            agentId={agentId}
            activeSessionId={controller.activeSessionId}
            onSelectSession={controller.setActiveSessionId}
            onNewSession={controller.handleNewSession}
            draftSession={controller.draftSession}
            onDeleteDraft={controller.handleDeleteDraft}
            projectId={projectId ?? undefined}
          />
        </div>
      )}

      <div className="relative flex-1 min-w-0 flex flex-col">
        <div className="flex-1 min-h-0 relative">
          {controller.draft &&
          controller.draftSession &&
          controller.activeSessionId === controller.draftSession.id ? (
            <ChatPanel
              draft={controller.draft}
              onDraftTextChange={controller.handleDraftTextChange}
              onDraftSend={controller.handleDraftSend}
              agentIdentity={agentConfig?.identity}
            />
          ) : controller.activeSessionId && !isDraftSessionId(controller.activeSessionId) ? (
            <ChatPanel
              sessionId={controller.activeSessionId}
              initialQueuedMessage={activeQueuedMessage}
              onInitialMessageHandled={controller.handleInitialMessageHandled}
              onInitialMessageFailed={controller.handleInitialMessageFailed}
              agentIdentity={agentConfig?.identity}
            />
          ) : (
            <div className="flex h-full items-center justify-center px-6 text-sm text-muted">
              {emptyStateCopy}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
