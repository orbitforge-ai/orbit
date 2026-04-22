import { useState, useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Bot, ChevronDown, Users } from 'lucide-react';
import * as Select from '@radix-ui/react-select';
import { projectsApi } from '../../api/projects';
import { useUiStore } from '../../store/uiStore';
import { ChatWorkspace, useChatWorkspaceController } from '../../components/chat';

export function ProjectChatTab({ projectId }: { projectId: string }) {
  const { setProjectTab } = useUiStore();
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);

  const { data: projectAgents = [] } = useQuery({
    queryKey: ['project-agents-meta', projectId],
    queryFn: () => projectsApi.listAgentsWithMeta(projectId),
  });

  // Pick default agent (is_default first, fall back to first agent)
  useEffect(() => {
    if (selectedAgentId) {
      const stillMember = projectAgents.some((entry) => entry.agent.id === selectedAgentId);
      if (!stillMember) {
        setSelectedAgentId(null);
      }
    }
    if (!selectedAgentId && projectAgents.length > 0) {
      const defaultAgent = projectAgents.find((entry) => entry.isDefault) ?? projectAgents[0];
      setSelectedAgentId(defaultAgent.agent.id);
    }
  }, [projectAgents, selectedAgentId]);

  const controller = useChatWorkspaceController({
    agentId: selectedAgentId,
    projectId,
    selectionMode: 'empty',
  });

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
      {selectedAgentId && (
        <div className="flex items-center gap-3 px-4 py-2 border-b border-edge bg-surface/30">
          <label className="text-[10px] uppercase tracking-wider text-muted shrink-0">
            Agent
          </label>
          <Select.Root value={selectedAgentId} onValueChange={setSelectedAgentId}>
            <Select.Trigger className="flex items-center justify-between gap-2 min-w-[220px] px-3 py-1.5 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent">
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
        </div>
      )}

      <div className="flex flex-1 min-h-0 min-w-0">
        {selectedAgentId ? (
          <ChatWorkspace
            agentId={selectedAgentId}
            projectId={projectId}
            controller={controller}
          />
        ) : (
          <div className="flex flex-1 items-center justify-center text-muted text-sm">
            Select an agent to start chatting
          </div>
        )}
      </div>
    </div>
  );
}
