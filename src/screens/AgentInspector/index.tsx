import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Bot,
  Save,
  X,
  Activity,
  Play,
  FolderOpen,
  Settings,
  LayoutDashboard,
  Clock,
  Zap,
} from 'lucide-react';
import { agentsApi } from '../../api/agents';
import { StatusBadge } from '../../components/StatusBadge';
import { Agent, CreateAgent, RunSummary } from '../../types';
import { invoke } from '@tauri-apps/api/core';
import { useUiStore } from '../../store/uiStore';
import { WorkspaceTab } from './WorkspaceTab';
import { ConfigTab } from './ConfigTab';
import { SchedulesTab } from './SchedulesTab';
import { AgentRunDialog } from './AgentRunDialog';
import { AgentRunView } from './AgentRunView';

const DEFAULT_AGENT_ID = 'default';

export function AgentInspector() {
  const { selectedAgentId } = useUiStore();

  const { data: agents = [] } = useQuery({
    queryKey: ['agents'],
    queryFn: agentsApi.list,
    refetchInterval: 5_000,
  });

  // Show create form when "__new__" is selected
  if (selectedAgentId === '__new__') {
    return <NewAgentView />;
  }

  // If an agent is selected, show its detail view
  if (selectedAgentId) {
    return <AgentDetail agentId={selectedAgentId} agents={agents} />;
  }

  // No agent selected — prompt
  return (
    <div className="flex items-center justify-center h-full text-[#64748b] text-sm">
      Select an agent from the sidebar
    </div>
  );
}

function NewAgentView() {
  const queryClient = useQueryClient();
  const { navigate, selectAgent } = useUiStore();
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
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
      const created = await agentsApi.create(payload);
      queryClient.invalidateQueries({ queryKey: ['agents'] });
      selectAgent(created.id);
    } catch {
      setSaving(false);
    }
  }

  return (
    <div className="flex items-center justify-center h-full">
      <div className="w-full max-w-md rounded-xl border border-[#2a2d3e] bg-[#1a1d27] p-6 space-y-4">
        <h3 className="text-base font-semibold text-white">Create New Agent</h3>
        <input
          type="text"
          placeholder="Agent name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          autoFocus
          className="w-full px-3 py-2 rounded-lg bg-[#0f1117] border border-[#2a2d3e] text-white text-sm focus:outline-none focus:border-[#6366f1]"
        />
        <input
          type="text"
          placeholder="Description (optional)"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          className="w-full px-3 py-2 rounded-lg bg-[#0f1117] border border-[#2a2d3e] text-white text-sm focus:outline-none focus:border-[#6366f1]"
        />
        <div className="flex items-center gap-2">
          <label className="text-xs text-[#64748b]">Max concurrent runs:</label>
          <input
            type="number"
            min={1}
            max={50}
            value={maxConcurrent}
            onChange={(e) => setMaxConcurrent(Number(e.target.value))}
            className="w-20 px-2 py-1.5 rounded-lg bg-[#0f1117] border border-[#2a2d3e] text-white text-sm focus:outline-none focus:border-[#6366f1]"
          />
        </div>
        <div className="flex gap-2 pt-1">
          <button
            onClick={handleSave}
            disabled={saving || !name.trim()}
            className="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] disabled:opacity-50 text-white text-sm font-medium"
          >
            <Save size={14} /> {saving ? 'Creating...' : 'Create Agent'}
          </button>
          <button
            onClick={() => navigate('dashboard')}
            className="flex items-center gap-1.5 px-4 py-2 rounded-lg text-[#64748b] hover:text-white text-sm"
          >
            <X size={14} /> Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

function AgentDetail({ agentId, agents }: { agentId: string; agents: Agent[] }) {
  const agent = agents.find((a) => a.id === agentId);
  const { agentTab, setAgentTab } = useUiStore();
  const [showRunDialog, setShowRunDialog] = useState(false);
  const [viewingRunId, setViewingRunId] = useState<string | null>(null);
  const queryClient = useQueryClient();

  if (!agent) {
    return (
      <div className="flex items-center justify-center h-full text-[#64748b] text-sm">
        Agent not found. Select one from the sidebar.
      </div>
    );
  }

  // If viewing a specific agent run, show the run view
  if (viewingRunId) {
    return <AgentRunView runId={viewingRunId} onBack={() => setViewingRunId(null)} />;
  }

  const tabs = [
    { id: 'overview' as const, label: 'Overview', icon: LayoutDashboard },
    { id: 'workspace' as const, label: 'Workspace', icon: FolderOpen },
    { id: 'config' as const, label: 'Config', icon: Settings },
    { id: 'schedules' as const, label: 'Schedules', icon: Clock },
  ];

  return (
    <div className="flex flex-col h-full">
      {/* Header with tabs */}
      <div className="border-b border-[#2a2d3e]">
        <div className="flex items-center justify-between px-6 pt-4 pb-0">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-full bg-[#6366f1]/20 flex items-center justify-center">
              <Bot size={18} className="text-[#818cf8]" />
            </div>
            <div>
              <h3 className="text-base font-semibold text-white">{agent.name}</h3>
              <div className="flex items-center gap-2">
                <StatusBadge state={agent.state} />
                {agent.description && (
                  <span className="text-xs text-[#64748b]">{agent.description}</span>
                )}
              </div>
            </div>
          </div>
          {agent.id !== DEFAULT_AGENT_ID && (
            <button
              onClick={() => setShowRunDialog(true)}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] text-white text-xs font-medium transition-colors"
            >
              <Play size={12} /> Run Agent
            </button>
          )}
        </div>

        {/* Tab bar */}
        <div className="flex gap-1 px-6 mt-3">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setAgentTab(tab.id)}
              className={`flex items-center gap-1.5 px-3 py-2 text-xs font-medium rounded-t-lg border-b-2 transition-colors ${
                agentTab === tab.id
                  ? 'border-[#6366f1] text-white bg-[#6366f1]/10'
                  : 'border-transparent text-[#64748b] hover:text-white'
              }`}
            >
              <tab.icon size={13} />
              {tab.label}
            </button>
          ))}
        </div>
      </div>

      {/* Tab content */}
      <div className="flex-1 overflow-hidden">
        {agentTab === 'overview' && (
          <OverviewContent
            agentId={agentId}
            agent={agent}
            onViewRun={(runId) => setViewingRunId(runId)}
          />
        )}
        {agentTab === 'workspace' && <WorkspaceTab agentId={agentId} />}
        {agentTab === 'config' && <ConfigTab agentId={agentId} />}
        {agentTab === 'schedules' && <SchedulesTab agentId={agentId} />}
      </div>

      {/* Run dialog */}
      <AgentRunDialog
        agentId={agentId}
        agentName={agent.name}
        open={showRunDialog}
        onClose={() => setShowRunDialog(false)}
        onRunStarted={(runId) => {
          queryClient.invalidateQueries({ queryKey: ['active-runs'] });
          setViewingRunId(runId);
        }}
      />
    </div>
  );
}

function OverviewContent({
  agentId,
  agent,
  onViewRun,
}: {
  agentId: string;
  agent: Agent;
  onViewRun: (runId: string) => void;
}) {
  const { openChatSession } = useUiStore();

  function handleRunClick(run: RunSummary) {
    if (run.chatSessionId) {
      openChatSession(run.chatSessionId);
    } else {
      onViewRun(run.id);
    }
  }

  function parseName(run: RunSummary) {
    if (run.taskName.includes('Pulse')) {
      return (
        <div className="flex items-center gap-1">
          <Zap size={16} className="text-[#f59e0b]" />
          <>{'Pulse'}</>
        </div>
      );
    } else {
      return run.taskName;
    }
  }

  const { data: recentRuns = [] } = useQuery<RunSummary[]>({
    queryKey: ['runs', 'agent', agentId],
    queryFn: () => invoke('list_runs', { limit: 20, offset: 0, stateFilter: null, taskId: null }),
    refetchInterval: 5_000,
    select: (runs: RunSummary[]) => runs.filter((r) => r.agentId === agentId),
  });

  const { data: activeRuns = [] } = useQuery<RunSummary[]>({
    queryKey: ['active-runs'],
    queryFn: () => invoke('get_active_runs'),
    refetchInterval: 3_000,
    select: (runs: RunSummary[]) => runs.filter((r) => r.agentId === agentId),
  });

  const successCount = recentRuns.filter((r) => r.state === 'success').length;
  const failureCount = recentRuns.filter((r) => r.state === 'failure').length;
  const totalCompleted = successCount + failureCount;
  const successRate = totalCompleted > 0 ? Math.round((successCount / totalCompleted) * 100) : null;
  const avgDuration =
    recentRuns.filter((r) => r.durationMs).length > 0
      ? Math.round(
          recentRuns.filter((r) => r.durationMs).reduce((sum, r) => sum + (r.durationMs ?? 0), 0) /
            recentRuns.filter((r) => r.durationMs).length
        )
      : null;

  return (
    <div className="p-6 space-y-6 overflow-y-auto h-full">
      {/* Stats */}
      <div className="grid grid-cols-4 gap-3">
        <StatCard label="Active runs" value={activeRuns.length.toString()} accent />
        <StatCard label="Max concurrent" value={agent.maxConcurrentRuns.toString()} />
        <StatCard label="Success rate" value={successRate !== null ? `${successRate}%` : '--'} />
        <StatCard
          label="Avg duration"
          value={avgDuration !== null ? `${(avgDuration / 1000).toFixed(1)}s` : '--'}
        />
      </div>

      {/* Active runs */}
      {activeRuns.length > 0 && (
        <div>
          <h4 className="text-sm font-semibold text-white mb-3">Currently Running</h4>
          <div className="space-y-2">
            {activeRuns.map((run) => (
              <div
                key={run.id}
                onClick={() => handleRunClick(run)}
                className="flex items-center gap-3 px-4 py-3 rounded-lg border border-[#2a2d3e] bg-[#1a1d27] cursor-pointer hover:border-[#4a4d6e]"
              >
                <Activity size={14} className="text-blue-400 animate-pulse" />
                <div className="flex-1 min-w-0">
                  <p className="text-sm text-white truncate">{parseName(run)}</p>
                  <p className="text-xs text-[#64748b]">
                    {run.trigger} &middot; started{' '}
                    {run.startedAt ? new Date(run.startedAt).toLocaleTimeString() : '...'}
                  </p>
                </div>
                <button
                  onClick={async (e) => {
                    e.stopPropagation();
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
            {recentRuns.slice(0, 20).map((run) => (
              <div
                key={run.id}
                onClick={() => handleRunClick(run)}
                className="flex items-center gap-3 px-4 py-2.5 rounded-lg hover:bg-[#1a1d27] cursor-pointer"
              >
                <StatusBadge state={run.state} />
                <p className="text-sm text-white flex-1 truncate">{parseName(run)}</p>
                <p className="text-xs text-[#64748b]">
                  {run.durationMs ? `${(run.durationMs / 1000).toFixed(1)}s` : '--'}
                </p>
                <p className="text-xs text-[#64748b]">
                  {run.createdAt ? new Date(run.createdAt).toLocaleString() : ''}
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
      <p className={`text-xl font-semibold ${accent ? 'text-[#818cf8]' : 'text-white'}`}>{value}</p>
    </div>
  );
}
