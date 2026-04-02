import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Bot, Plus, X } from 'lucide-react';
import { projectsApi } from '../../api/projects';
import { agentsApi } from '../../api/agents';
import { Agent } from '../../types';
import { useUiStore } from '../../store/uiStore';

export function ProjectAgentsTab({ projectId }: { projectId: string }) {
  const queryClient = useQueryClient();
  const { selectAgent } = useUiStore();
  const [adding, setAdding] = useState(false);

  const { data: projectAgents = [] } = useQuery<Agent[]>({
    queryKey: ['project-agents', projectId],
    queryFn: () => projectsApi.listAgents(projectId),
  });

  const { data: allAgents = [] } = useQuery<Agent[]>({
    queryKey: ['agents'],
    queryFn: agentsApi.list,
    enabled: adding,
  });

  const projectAgentIds = new Set(projectAgents.map((a) => a.id));
  const addableAgents = allAgents.filter((a) => !projectAgentIds.has(a.id));

  async function handleAdd(agentId: string) {
    await projectsApi.addAgent(projectId, agentId, projectAgents.length === 0);
    queryClient.invalidateQueries({ queryKey: ['project-agents', projectId] });
    setAdding(false);
  }

  async function handleRemove(agentId: string) {
    await projectsApi.removeAgent(projectId, agentId);
    queryClient.invalidateQueries({ queryKey: ['project-agents', projectId] });
  }

  return (
    <div className="flex flex-col h-full p-4 gap-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold text-white">
          Assigned Agents
          <span className="ml-2 text-xs text-muted font-normal">({projectAgents.length})</span>
        </h3>
        <button
          onClick={() => setAdding(!adding)}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover text-white text-xs font-medium transition-colors"
        >
          <Plus size={13} />
          Add Agent
        </button>
      </div>

      {/* Add agent picker */}
      {adding && (
        <div className="rounded-xl border border-edge bg-surface p-3 space-y-2">
          <p className="text-xs text-muted font-medium">Select an agent to add:</p>
          {addableAgents.length === 0 ? (
            <p className="text-xs text-muted italic">All agents are already assigned.</p>
          ) : (
            addableAgents.map((agent) => (
              <button
                key={agent.id}
                onClick={() => handleAdd(agent.id)}
                className="w-full flex items-center gap-3 px-3 py-2.5 rounded-lg border border-edge bg-panel hover:border-accent hover:bg-accent/10 transition-colors text-left"
              >
                <Bot size={14} className="text-muted shrink-0" />
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium text-white">{agent.name}</p>
                  {agent.description && (
                    <p className="text-xs text-muted truncate">{agent.description}</p>
                  )}
                </div>
              </button>
            ))
          )}
          <button
            onClick={() => setAdding(false)}
            className="text-xs text-muted hover:text-white transition-colors"
          >
            Cancel
          </button>
        </div>
      )}

      {/* Assigned agents */}
      {projectAgents.length === 0 ? (
        <div className="flex flex-col items-center justify-center flex-1 gap-2 text-muted text-sm">
          <Bot size={28} className="opacity-30" />
          <span>No agents assigned yet</span>
          <p className="text-xs text-center max-w-xs">
            Add agents to this project so they can access the shared workspace.
          </p>
        </div>
      ) : (
        <ul className="space-y-2">
          {projectAgents.map((agent) => (
            <li
              key={agent.id}
              className="flex items-center gap-3 px-4 py-3 rounded-xl border border-edge bg-surface"
            >
              <div
                className={`w-2 h-2 rounded-full shrink-0 ${
                  agent.state === 'idle' ? 'bg-emerald-400' : 'bg-slate-500'
                }`}
              />
              <button
                onClick={() => selectAgent(agent.id)}
                className="flex-1 min-w-0 text-left"
              >
                <p className="text-sm font-medium text-white hover:text-accent-hover transition-colors">
                  {agent.name}
                </p>
                {agent.description && (
                  <p className="text-xs text-muted truncate">{agent.description}</p>
                )}
              </button>
              <button
                onClick={() => handleRemove(agent.id)}
                className="p-1.5 rounded-md text-muted hover:text-red-400 hover:bg-red-400/10 transition-colors"
                title="Remove from project"
              >
                <X size={13} />
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
