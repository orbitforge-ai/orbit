import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Bot, Trash2 } from "lucide-react";
import { agentsApi } from "../../api/agents";
import { StatusBadge } from "../../components/StatusBadge";
import { Agent } from "../../types";

export function AgentInspector() {
  const queryClient = useQueryClient();

  const { data: agents = [], isLoading } = useQuery({
    queryKey: ["agents"],
    queryFn: agentsApi.list,
    refetchInterval: 10_000,
  });

  async function handleDelete(agent: Agent) {
    if (!confirm(`Delete agent "${agent.name}"?`)) return;
    await agentsApi.delete(agent.id);
    queryClient.invalidateQueries({ queryKey: ["agents"] });
  }

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center justify-between px-6 py-4 border-b border-[#2a2d3e]">
        <h2 className="text-lg font-semibold text-white">Agents</h2>
      </div>

      <div className="flex-1 overflow-y-auto p-4">
        {isLoading && (
          <div className="text-center py-8 text-[#64748b] text-sm">Loading…</div>
        )}
        {!isLoading && agents.length === 0 && (
          <div className="text-center py-16 text-[#64748b] text-sm">
            No agents configured
          </div>
        )}

        <div className="grid grid-cols-1 gap-3">
          {agents.map((agent) => (
            <AgentCard key={agent.id} agent={agent} onDelete={handleDelete} />
          ))}
        </div>
      </div>
    </div>
  );
}

function AgentCard({
  agent,
  onDelete,
}: {
  agent: Agent;
  onDelete: (a: Agent) => void;
}) {
  const isDefault = agent.id === "01HZDEFAULTDEFAULTDEFAULTDA";

  return (
    <div className="flex items-center gap-4 px-5 py-4 rounded-xl border border-[#2a2d3e] bg-[#1a1d27]">
      <div className="w-10 h-10 rounded-full bg-[#6366f1]/20 flex items-center justify-center flex-shrink-0">
        <Bot size={18} className="text-[#818cf8]" />
      </div>

      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <p className="text-sm font-semibold text-white">{agent.name}</p>
          {isDefault && (
            <span className="text-[10px] px-1.5 py-0.5 rounded bg-[#2a2d3e] text-[#64748b]">
              Default
            </span>
          )}
          <StatusBadge state={agent.state} />
        </div>
        <p className="text-xs text-[#64748b] mt-0.5">
          Max {agent.maxConcurrentRuns} concurrent runs
          {agent.heartbeatAt && (
            <> · Last active {new Date(agent.heartbeatAt).toLocaleString()}</>
          )}
        </p>
      </div>

      {!isDefault && (
        <button
          onClick={() => onDelete(agent)}
          className="p-1.5 rounded text-[#64748b] hover:text-red-400 hover:bg-red-500/10 transition-colors"
        >
          <Trash2 size={14} />
        </button>
      )}
    </div>
  );
}
