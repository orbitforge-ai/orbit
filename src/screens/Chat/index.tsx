import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import * as Select from '@radix-ui/react-select';
import { Bot, ChevronDown } from 'lucide-react';
import { ChatWorkspace, useChatWorkspaceController } from '../../components/chat';
import { agentsApi } from '../../api/agents';
import { useUiStore } from '../../store/uiStore';

export function ChatScreen() {
  const { selectedAgentId: preferredAgentId, pendingChatSessionId, clearPendingChatSession } =
    useUiStore();
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);

  const { data: agents = [] } = useQuery({
    queryKey: ['agents'],
    queryFn: agentsApi.list,
  });

  useEffect(() => {
    if (selectedAgentId && !agents.some((agent) => agent.id === selectedAgentId)) {
      setSelectedAgentId(null);
    }

    if (preferredAgentId && agents.some((agent) => agent.id === preferredAgentId)) {
      if (selectedAgentId !== preferredAgentId) {
        setSelectedAgentId(preferredAgentId);
      }
      return;
    }

    if (!selectedAgentId && agents.length > 0) {
      setSelectedAgentId(agents[0].id);
    }
  }, [agents, preferredAgentId, selectedAgentId]);

  const controller = useChatWorkspaceController({
    agentId: selectedAgentId,
    pendingSessionId: pendingChatSessionId,
    selectionMode: 'empty',
    onPendingSessionHandled: clearPendingChatSession,
  });

  return (
    <div className="flex h-full">
      {selectedAgentId ? (
        <ChatWorkspace
          agentId={selectedAgentId}
          controller={controller}
          sidebarHeader={
            <div className="px-4 py-3 border-b border-edge">
              <label className="text-[10px] uppercase tracking-wider text-muted mb-1.5 block">
                Agent
              </label>
              <Select.Root value={selectedAgentId} onValueChange={setSelectedAgentId}>
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
            </div>
          }
        />
      ) : (
        <div className="flex-1 flex items-center justify-center text-muted text-sm">
          Select an agent to start chatting
        </div>
      )}
    </div>
  );
}
