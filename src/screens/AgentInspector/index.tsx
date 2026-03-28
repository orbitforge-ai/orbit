import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Bot, Edit2, Plus, Save, Trash2, X, Activity } from "lucide-react";
import { agentsApi } from "../../api/agents";
import { StatusBadge } from "../../components/StatusBadge";
import { Agent, CreateAgent, RunSummary, UpdateAgent } from "../../types";
import { invoke } from "@tauri-apps/api/core";

const DEFAULT_AGENT_ID = "01HZDEFAULTDEFAULTDEFAULTDA";

export function AgentInspector() {
  const queryClient = useQueryClient();
  const [showCreateForm, setShowCreateForm] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [selectedAgent, setSelectedAgent] = useState<string | null>(null);

  const { data: agents = [], isLoading } = useQuery({
    queryKey: ["agents"],
    queryFn: agentsApi.list,
    refetchInterval: 5_000,
  });

  async function handleDelete(agent: Agent) {
    if (!confirm(`Delete agent "${agent.name}"?`)) return;
    await agentsApi.delete(agent.id);
    queryClient.invalidateQueries({ queryKey: ["agents"] });
    if (selectedAgent === agent.id) setSelectedAgent(null);
  }

  return (
    <div className="flex h-full">
      {/* Left: Agent list */}
      <div className="w-[380px] flex flex-col border-r border-[#2a2d3e]">
        <div className="flex items-center justify-between px-6 py-4 border-b border-[#2a2d3e]">
          <h2 className="text-lg font-semibold text-white">Agents</h2>
          <button
            onClick={() => setShowCreateForm(true)}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] text-white text-xs font-medium transition-colors"
          >
            <Plus size={12} /> New Agent
          </button>
        </div>

        <div className="flex-1 overflow-y-auto p-4 space-y-3">
          {isLoading && (
            <div className="text-center py-8 text-[#64748b] text-sm">Loading...</div>
          )}
          {!isLoading && agents.length === 0 && (
            <div className="text-center py-16 text-[#64748b] text-sm">
              Agents help organize and limit task concurrency. Create one to get started.
            </div>
          )}

          {showCreateForm && (
            <CreateAgentForm
              onCreated={() => {
                setShowCreateForm(false);
                queryClient.invalidateQueries({ queryKey: ["agents"] });
              }}
              onCancel={() => setShowCreateForm(false)}
            />
          )}

          {agents.map((agent) =>
            editingId === agent.id ? (
              <EditAgentForm
                key={agent.id}
                agent={agent}
                onSaved={() => {
                  setEditingId(null);
                  queryClient.invalidateQueries({ queryKey: ["agents"] });
                }}
                onCancel={() => setEditingId(null)}
              />
            ) : (
              <AgentCard
                key={agent.id}
                agent={agent}
                selected={selectedAgent === agent.id}
                onSelect={() => setSelectedAgent(agent.id)}
                onEdit={() => setEditingId(agent.id)}
                onDelete={handleDelete}
              />
            )
          )}
        </div>
      </div>

      {/* Right: Agent detail */}
      <div className="flex-1 overflow-y-auto">
        {selectedAgent ? (
          <AgentDetail agentId={selectedAgent} agents={agents} />
        ) : (
          <div className="flex items-center justify-center h-full text-[#64748b] text-sm">
            Select an agent to view details
          </div>
        )}
      </div>
    </div>
  );
}

function AgentCard({
  agent,
  selected,
  onSelect,
  onEdit,
  onDelete,
}: {
  agent: Agent;
  selected: boolean;
  onSelect: () => void;
  onEdit: () => void;
  onDelete: (a: Agent) => void;
}) {
  const isDefault = agent.id === DEFAULT_AGENT_ID;

  return (
    <div
      onClick={onSelect}
      className={`flex items-center gap-4 px-5 py-4 rounded-xl border cursor-pointer transition-colors ${
        selected
          ? "border-[#6366f1] bg-[#6366f1]/10"
          : "border-[#2a2d3e] bg-[#1a1d27] hover:border-[#4a4d6e]"
      }`}
    >
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
          {agent.description && <> &middot; {agent.description}</>}
        </p>
      </div>

      <div className="flex items-center gap-1.5 flex-shrink-0">
        <button
          onClick={(e) => { e.stopPropagation(); onEdit(); }}
          className="p-1.5 rounded text-[#64748b] hover:text-[#818cf8] hover:bg-[#6366f1]/10 transition-colors"
        >
          <Edit2 size={13} />
        </button>
        {!isDefault && (
          <button
            onClick={(e) => { e.stopPropagation(); onDelete(agent); }}
            className="p-1.5 rounded text-[#64748b] hover:text-red-400 hover:bg-red-500/10 transition-colors"
          >
            <Trash2 size={13} />
          </button>
        )}
      </div>
    </div>
  );
}

function CreateAgentForm({
  onCreated,
  onCancel,
}: {
  onCreated: () => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [maxConcurrent, setMaxConcurrent] = useState(5);
  const [saving, setSaving] = useState(false);

  async function handleSave() {
    if (!name.trim()) return;
    setSaving(true);
    try {
      const payload: CreateAgent = {
        name: name.trim(),
        description: description.trim() || undefined,
        maxConcurrentRuns: maxConcurrent,
      };
      await agentsApi.create(payload);
      onCreated();
    } catch {
      setSaving(false);
    }
  }

  return (
    <div className="rounded-xl border border-[#6366f1] bg-[#1a1d27] p-4 space-y-3">
      <input type="text" placeholder="Agent name" value={name}
        onChange={e => setName(e.target.value)} autoFocus
        className="w-full px-3 py-2 rounded-lg bg-[#0f1117] border border-[#2a2d3e] text-white text-sm focus:outline-none focus:border-[#6366f1]" />
      <input type="text" placeholder="Description (optional)" value={description}
        onChange={e => setDescription(e.target.value)}
        className="w-full px-3 py-2 rounded-lg bg-[#0f1117] border border-[#2a2d3e] text-white text-sm focus:outline-none focus:border-[#6366f1]" />
      <div className="flex items-center gap-2">
        <label className="text-xs text-[#64748b]">Max concurrent:</label>
        <input type="number" min={1} max={50} value={maxConcurrent}
          onChange={e => setMaxConcurrent(Number(e.target.value))}
          className="w-20 px-2 py-1.5 rounded-lg bg-[#0f1117] border border-[#2a2d3e] text-white text-sm focus:outline-none focus:border-[#6366f1]" />
      </div>
      <div className="flex gap-2 pt-1">
        <button onClick={handleSave} disabled={saving || !name.trim()}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] disabled:opacity-50 text-white text-xs font-medium">
          <Save size={12} /> {saving ? "Saving..." : "Create"}
        </button>
        <button onClick={onCancel}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-[#64748b] hover:text-white text-xs">
          <X size={12} /> Cancel
        </button>
      </div>
    </div>
  );
}

function EditAgentForm({
  agent,
  onSaved,
  onCancel,
}: {
  agent: Agent;
  onSaved: () => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(agent.name);
  const [description, setDescription] = useState(agent.description ?? "");
  const [maxConcurrent, setMaxConcurrent] = useState(agent.maxConcurrentRuns);
  const [saving, setSaving] = useState(false);

  async function handleSave() {
    if (!name.trim()) return;
    setSaving(true);
    try {
      const payload: UpdateAgent = {
        name: name.trim(),
        description: description.trim() || undefined,
        maxConcurrentRuns: maxConcurrent,
      };
      await agentsApi.update(agent.id, payload);
      onSaved();
    } catch {
      setSaving(false);
    }
  }

  return (
    <div className="rounded-xl border border-[#6366f1] bg-[#1a1d27] p-4 space-y-3">
      <input type="text" value={name} onChange={e => setName(e.target.value)} autoFocus
        className="w-full px-3 py-2 rounded-lg bg-[#0f1117] border border-[#2a2d3e] text-white text-sm focus:outline-none focus:border-[#6366f1]" />
      <input type="text" placeholder="Description" value={description}
        onChange={e => setDescription(e.target.value)}
        className="w-full px-3 py-2 rounded-lg bg-[#0f1117] border border-[#2a2d3e] text-white text-sm focus:outline-none focus:border-[#6366f1]" />
      <div className="flex items-center gap-2">
        <label className="text-xs text-[#64748b]">Max concurrent:</label>
        <input type="number" min={1} max={50} value={maxConcurrent}
          onChange={e => setMaxConcurrent(Number(e.target.value))}
          className="w-20 px-2 py-1.5 rounded-lg bg-[#0f1117] border border-[#2a2d3e] text-white text-sm focus:outline-none focus:border-[#6366f1]" />
      </div>
      <div className="flex gap-2 pt-1">
        <button onClick={handleSave} disabled={saving || !name.trim()}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] disabled:opacity-50 text-white text-xs font-medium">
          <Save size={12} /> {saving ? "Saving..." : "Save"}
        </button>
        <button onClick={onCancel}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-[#64748b] hover:text-white text-xs">
          <X size={12} /> Cancel
        </button>
      </div>
    </div>
  );
}

function AgentDetail({ agentId, agents }: { agentId: string; agents: Agent[] }) {
  const agent = agents.find(a => a.id === agentId);

  const { data: recentRuns = [] } = useQuery<RunSummary[]>({
    queryKey: ["runs", "agent", agentId],
    queryFn: () => invoke("list_runs", { limit: 20, offset: 0, stateFilter: null, taskId: null }),
    refetchInterval: 5_000,
    select: (runs: RunSummary[]) => runs.filter(r => r.agentId === agentId),
  });

  const { data: activeRuns = [] } = useQuery<RunSummary[]>({
    queryKey: ["active-runs"],
    queryFn: () => invoke("get_active_runs"),
    refetchInterval: 3_000,
    select: (runs: RunSummary[]) => runs.filter(r => r.agentId === agentId),
  });

  if (!agent) return null;

  const successCount = recentRuns.filter(r => r.state === "success").length;
  const failureCount = recentRuns.filter(r => r.state === "failure").length;
  const totalCompleted = successCount + failureCount;
  const successRate = totalCompleted > 0 ? Math.round((successCount / totalCompleted) * 100) : null;
  const avgDuration = recentRuns.filter(r => r.durationMs).length > 0
    ? Math.round(recentRuns.filter(r => r.durationMs).reduce((sum, r) => sum + (r.durationMs ?? 0), 0) / recentRuns.filter(r => r.durationMs).length)
    : null;

  return (
    <div className="p-6 space-y-6">
      {/* Header */}
      <div className="flex items-center gap-4">
        <div className="w-14 h-14 rounded-full bg-[#6366f1]/20 flex items-center justify-center">
          <Bot size={24} className="text-[#818cf8]" />
        </div>
        <div>
          <h3 className="text-lg font-semibold text-white">{agent.name}</h3>
          <div className="flex items-center gap-2 mt-0.5">
            <StatusBadge state={agent.state} />
            {agent.description && (
              <span className="text-xs text-[#64748b]">{agent.description}</span>
            )}
          </div>
        </div>
      </div>

      {/* Stats */}
      <div className="grid grid-cols-4 gap-3">
        <StatCard label="Active runs" value={activeRuns.length.toString()} accent />
        <StatCard label="Max concurrent" value={agent.maxConcurrentRuns.toString()} />
        <StatCard label="Success rate" value={successRate !== null ? `${successRate}%` : "--"} />
        <StatCard label="Avg duration" value={avgDuration !== null ? `${(avgDuration / 1000).toFixed(1)}s` : "--"} />
      </div>

      {/* Active runs */}
      {activeRuns.length > 0 && (
        <div>
          <h4 className="text-sm font-semibold text-white mb-3">Currently Running</h4>
          <div className="space-y-2">
            {activeRuns.map(run => (
              <div key={run.id} className="flex items-center gap-3 px-4 py-3 rounded-lg border border-[#2a2d3e] bg-[#1a1d27]">
                <Activity size={14} className="text-blue-400 animate-pulse" />
                <div className="flex-1 min-w-0">
                  <p className="text-sm text-white truncate">{run.taskName}</p>
                  <p className="text-xs text-[#64748b]">{run.trigger} &middot; started {run.startedAt ? new Date(run.startedAt).toLocaleTimeString() : "..."}</p>
                </div>
                <button
                  onClick={async () => {
                    await agentsApi.cancelRun(run.id);
                  }}
                  className="px-2 py-1 rounded text-xs text-red-400 hover:bg-red-500/10 border border-red-500/30"
                >
                  Stop
                </button>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Recent runs */}
      <div>
        <h4 className="text-sm font-semibold text-white mb-3">Recent Runs</h4>
        {recentRuns.length === 0 ? (
          <p className="text-sm text-[#64748b]">No runs yet for this agent.</p>
        ) : (
          <div className="space-y-1">
            {recentRuns.slice(0, 20).map(run => (
              <div key={run.id} className="flex items-center gap-3 px-4 py-2.5 rounded-lg hover:bg-[#1a1d27]">
                <StatusBadge state={run.state} />
                <p className="text-sm text-white flex-1 truncate">{run.taskName}</p>
                <p className="text-xs text-[#64748b]">
                  {run.durationMs ? `${(run.durationMs / 1000).toFixed(1)}s` : "--"}
                </p>
                <p className="text-xs text-[#64748b]">
                  {run.createdAt ? new Date(run.createdAt).toLocaleString() : ""}
                </p>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function StatCard({ label, value, accent }: { label: string; value: string; accent?: boolean }) {
  return (
    <div className="rounded-xl border border-[#2a2d3e] bg-[#1a1d27] p-4">
      <p className="text-xs text-[#64748b] mb-1">{label}</p>
      <p className={`text-xl font-semibold ${accent ? "text-[#818cf8]" : "text-white"}`}>{value}</p>
    </div>
  );
}
