import { useEffect, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import {
  Activity,
  Bot,
  Clock,
  FolderOpen,
  GitBranch,
  History,
  MessageSquare,
  Play,
  Radio,
  Save,
  Settings,
  Sparkles,
  X,
  Zap,
} from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { agentsApi } from '../../api/agents';
import { chatApi } from '../../api/chat';
import { workspaceApi } from '../../api/workspace';
import { StatusBadge } from '../../components/StatusBadge';
import { InlineEdit } from '../../components/InlineEdit';
import { ChatWorkspace, useChatWorkspaceController } from '../../components/chat';
import { useUiStore } from '../../store/uiStore';
import { Agent, ChatSession, CreateAgent, RunSummary } from '../../types';
import { WorkspaceTab } from './WorkspaceTab';
import { ConfigTab } from './ConfigTab';
import { SchedulesTab } from './SchedulesTab';
import { BusTab } from './BusTab';
import { SkillsTab } from './SkillsTab';
import { AgentRunDialog } from './AgentRunDialog';
import { AgentRunView } from './AgentRunView';
import { AgentIdentitySection } from './AgentIdentitySection';
import { RoleSelector, ROLE_ICON_MAP } from './RoleSelector';
import { getDefaultAgentIdentity } from '../../lib/agentIdentity';
import {
  DEFAULT_ROLE_ID,
  getRoleSystemInstructions,
  resolveRole,
} from '../../lib/agentRoles';
import { AgentRoleSelect } from './Header/AgentRoleSelect';

type ActivityItem =
  | { key: string; kind: 'session'; timestamp: number; session: ChatSession }
  | { key: string; kind: 'run'; timestamp: number; run: RunSummary };

export function AgentInspector() {
  const { selectedAgentId } = useUiStore();

  const { data: agents = [] } = useQuery({
    queryKey: ['agents'],
    queryFn: agentsApi.list,
    refetchInterval: 5_000,
  });

  if (selectedAgentId === '__new__') {
    return <NewAgentView />;
  }

  if (selectedAgentId) {
    return <AgentDetail agentId={selectedAgentId} agents={agents} />;
  }

  return (
    <div className="flex items-center justify-center h-full text-muted text-sm">
      Select an agent from the sidebar
    </div>
  );
}

function NewAgentView() {
  const { navigate, selectAgent } = useUiStore();
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [maxConcurrent, setMaxConcurrent] = useState(5);
  const [identity, setIdentity] = useState(getDefaultAgentIdentity());
  const [roleId, setRoleId] = useState(DEFAULT_ROLE_ID);
  const [saving, setSaving] = useState(false);

  async function handleSave() {
    if (!name.trim()) return;
    setSaving(true);
    try {
      const payload: CreateAgent = {
        name: name.trim(),
        description: description.trim() || undefined,
        maxConcurrentRuns: maxConcurrent,
        identity,
        roleId,
        roleSystemInstructions: getRoleSystemInstructions(roleId),
      };
      const created = await agentsApi.create(payload);
      selectAgent(created.id);
    } catch {
      setSaving(false);
    }
  }

  return (
    <div className="flex items-center justify-center h-full overflow-y-auto py-6">
      <div className="w-full max-w-lg rounded-xl border border-edge bg-surface p-6 space-y-4">
        <h3 className="text-base font-semibold text-white">Create New Agent</h3>
        <input
          type="text"
          placeholder="Agent name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          autoFocus
          className="w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent"
        />
        <input
          type="text"
          placeholder="Description (optional)"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          className="w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent"
        />
        <div className="space-y-2">
          <label className="text-xs text-muted">Role</label>
          <RoleSelector selected={roleId} onSelect={setRoleId} mode="full" />
        </div>
        <div className="flex items-center gap-2">
          <label className="text-xs text-muted">Max concurrent runs:</label>
          <input
            type="number"
            min={1}
            max={50}
            value={maxConcurrent}
            onChange={(e) => setMaxConcurrent(Number(e.target.value))}
            className="w-20 px-2 py-1.5 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent"
          />
        </div>
        <AgentIdentitySection
          identity={identity}
          onChange={setIdentity}
          agentName={name.trim() || 'this agent'}
        />
        <div className="flex gap-2 pt-1">
          <button
            onClick={handleSave}
            disabled={saving || !name.trim()}
            className="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-50 text-white text-sm font-medium"
          >
            <Save size={14} /> {saving ? 'Creating...' : 'Create Agent'}
          </button>
          <button
            onClick={() => navigate('dashboard')}
            className="flex items-center gap-1.5 px-4 py-2 rounded-lg text-muted hover:text-white text-sm"
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
  const { agentTab, setAgentTab, pendingChatSessionId, clearPendingChatSession } = useUiStore();
  const [showRunDialog, setShowRunDialog] = useState(false);
  const [viewingRunId, setViewingRunId] = useState<string | null>(null);
  const [chatSidebarCollapsed, setChatSidebarCollapsed] = useState(false);
  const queryClient = useQueryClient();

  const [dirtyTabs, setDirtyTabs] = useState<Record<string, boolean>>({});
  const [savingTab] = useState<string | null>(null);

  const configSaveRef = useRef<{ triggerSave: () => void } | null>(null);
  const schedulesSaveRef = useRef<{ triggerSave: () => void } | null>(null);

  useEffect(() => {
    setViewingRunId(null);
    setChatSidebarCollapsed(false);
  }, [agentId]);

  const { data: recentRuns = [] } = useQuery<RunSummary[]>({
    queryKey: ['runs', 'agent', agentId],
    queryFn: () => invoke('list_runs', { limit: 20, offset: 0, stateFilter: null, taskId: null }),
    refetchInterval: 5_000,
    select: (runs: RunSummary[]) => runs.filter((r) => r.agentId === agentId && !r.isSubAgent),
  });

  const { data: activeRuns = [] } = useQuery<RunSummary[]>({
    queryKey: ['active-runs'],
    queryFn: () => invoke('get_active_runs'),
    refetchInterval: 3_000,
    select: (runs: RunSummary[]) => runs.filter((r) => r.agentId === agentId && !r.isSubAgent),
  });

  const { data: chatSessions = [] } = useQuery<ChatSession[]>({
    queryKey: ['chat-sessions', agentId, false],
    queryFn: () => chatApi.listSessions(agentId, false),
    refetchInterval: 5_000,
  });

  const { data: agentConfig } = useQuery({
    queryKey: ['agent-config', agentId],
    queryFn: () => workspaceApi.getConfig(agentId),
    staleTime: 60_000,
  });

  const chatController = useChatWorkspaceController({
    agentId,
    pendingSessionId: pendingChatSessionId,
    selectionMode: 'latest-user-chat',
    onPendingSessionHandled: clearPendingChatSession,
  });

  async function handleInlineSave(field: 'name' | 'description', value: string) {
    await agentsApi.update(agentId, { [field]: value || undefined });
    queryClient.invalidateQueries({ queryKey: ['agents'] });
  }

  async function handleRoleChange(newRoleId: string) {
    if (!agentConfig) return;
    const updated = {
      ...agentConfig,
      roleId: newRoleId,
      roleSystemInstructions: getRoleSystemInstructions(newRoleId),
    };
    await workspaceApi.updateConfig(agentId, updated);
    queryClient.invalidateQueries({ queryKey: ['agent-config', agentId] });
  }

  function handleDirtyChange(tab: string, isDirty: boolean) {
    setDirtyTabs((prev) => ({ ...prev, [tab]: isDirty }));
  }

  function handleHeaderSave() {
    if (agentTab === 'config' && configSaveRef.current) {
      configSaveRef.current.triggerSave();
    } else if (agentTab === 'schedules' && schedulesSaveRef.current) {
      schedulesSaveRef.current.triggerSave();
    }
  }

  function handleOpenSession(sessionId: string) {
    setAgentTab('chat');
    setViewingRunId(null);
    chatController.setActiveSessionId(sessionId);
  }

  function handleRunClick(run: RunSummary) {
    if (run.chatSessionId) {
      handleOpenSession(run.chatSessionId);
    } else {
      setViewingRunId(run.id);
    }
  }

  const hasDirtyChanges = dirtyTabs[agentTab] === true;
  const isSaveableTab = agentTab === 'config' || agentTab === 'schedules';

  const successCount = recentRuns.filter((run) => run.state === 'success').length;
  const failureCount = recentRuns.filter((run) => run.state === 'failure').length;
  const totalCompleted = successCount + failureCount;
  const successRate = totalCompleted > 0 ? Math.round((successCount / totalCompleted) * 100) : null;
  const durationRuns = recentRuns.filter((run) => run.durationMs);
  const avgDuration =
    durationRuns.length > 0
      ? Math.round(
          durationRuns.reduce((sum, run) => sum + (run.durationMs ?? 0), 0) / durationRuns.length
        )
      : null;

  const activityItems: ActivityItem[] = [
    ...chatSessions.map((session) => ({
      key: `session:${session.id}`,
      kind: 'session' as const,
      timestamp: new Date(session.updatedAt).getTime(),
      session,
    })),
    ...recentRuns.map((run) => ({
      key: `run:${run.id}`,
      kind: 'run' as const,
      timestamp: new Date(run.createdAt).getTime(),
      run,
    })),
  ]
    .sort((a, b) => b.timestamp - a.timestamp)
    .slice(0, 10);

  if (!agent) {
    return (
      <div className="flex items-center justify-center h-full text-muted text-sm">
        Agent not found. Select one from the sidebar.
      </div>
    );
  }

  if (viewingRunId) {
    return <AgentRunView runId={viewingRunId} onBack={() => setViewingRunId(null)} />;
  }

  const tabs = [
    { id: 'chat' as const, label: 'Chat', icon: MessageSquare },
    { id: 'workspace' as const, label: 'Workspace', icon: FolderOpen },
    { id: 'config' as const, label: 'Config', icon: Settings },
    { id: 'skills' as const, label: 'Skills', icon: Sparkles },
    { id: 'schedules' as const, label: 'Schedules', icon: Clock },
    { id: 'bus' as const, label: 'Bus', icon: Radio },
  ];

  return (
    <div className="flex flex-col h-full">
      <div className="border-b border-edge">
        <div className="flex items-start justify-between gap-4 px-6 pt-4 pb-3">
          <div className="flex min-w-0 items-start gap-3">
            {(() => {
              const role = resolveRole(agentConfig?.roleId);
              const AgentIcon = ROLE_ICON_MAP[role.icon] ?? Bot;
              return (
                <div className="mt-0.5 flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-accent/20">
                  <AgentIcon
                    size={18}
                    className={agentConfig?.roleId ? role.color : 'text-accent-hover'}
                  />
                </div>
              );
            })()}
            <div className="min-w-0">
              <InlineEdit
                value={agent.name}
                onSave={(value) => handleInlineSave('name', value)}
                as="h3"
                className="text-base font-semibold text-white"
                inputClassName="text-base font-semibold text-white"
              />
              <div className="mt-1 flex min-w-0 items-center gap-2">
                <StatusBadge state={agent.state} />
                <InlineEdit
                  value={agent.description ?? ''}
                  placeholder="Add description"
                  onSave={(value) => handleInlineSave('description', value)}
                  className="truncate text-xs text-muted"
                  inputClassName="text-xs text-muted"
                />
              </div>
              <div className="mt-3 flex flex-wrap gap-2">
                {agentConfig && (
                  <AgentRoleSelect
                    agentConfig={agentConfig}
                    handleRoleChange={handleRoleChange}
                  />
                )}
                <HeaderStatChip
                  label="Active"
                  value={activeRuns.length.toString()}
                  accent={activeRuns.length > 0}
                />
                <HeaderStatChip label="Max concurrent" value={agent.maxConcurrentRuns.toString()} />
                <HeaderStatChip
                  label="Success"
                  value={successRate !== null ? `${successRate}%` : '--'}
                />
                <HeaderStatChip
                  label="Avg duration"
                  value={avgDuration !== null ? `${(avgDuration / 1000).toFixed(1)}s` : '--'}
                />
              </div>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <DropdownMenu.Root>
              <DropdownMenu.Trigger asChild>
                <button
                  className="rounded-lg border border-edge bg-surface p-2 text-muted transition-colors hover:text-white hover:border-edge-hover"
                  title="Recent activity"
                  aria-label="Open recent activity"
                >
                  <History size={14} />
                </button>
              </DropdownMenu.Trigger>
              <DropdownMenu.Portal>
                <DropdownMenu.Content
                  align="end"
                  sideOffset={8}
                  className="z-50 w-[320px] rounded-xl border border-edge bg-surface p-2 shadow-xl"
                >
                  <div className="px-2 py-1.5">
                    <p className="text-xs font-semibold text-white">Recent Activity</p>
                    <p className="text-[11px] text-muted">Chats and runs for this agent.</p>
                  </div>

                  <div className="mt-1 max-h-[360px] overflow-y-auto">
                    {activityItems.length === 0 ? (
                      <div className="px-2 py-6 text-center text-xs text-muted">
                        No recent activity yet.
                      </div>
                    ) : (
                      activityItems.map((item) => (
                        <DropdownMenu.Item
                          key={item.key}
                          onSelect={() => {
                            if (item.kind === 'session') {
                              handleOpenSession(item.session.id);
                            } else {
                              handleRunClick(item.run);
                            }
                          }}
                          className="flex cursor-pointer items-center gap-2 rounded-lg px-2 py-2 outline-none transition-colors data-[highlighted]:bg-accent/10"
                        >
                          {item.kind === 'session' ? (
                            <>
                              <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-background text-muted">
                                <SessionTypeIcon sessionType={item.session.sessionType} />
                              </div>
                              <div className="min-w-0 flex-1">
                                <p className="truncate text-sm text-white">{item.session.title}</p>
                                <p className="text-[11px] text-muted">
                                  {formatSessionType(item.session)} ·{' '}
                                  {formatActivityTime(item.session.updatedAt)}
                                </p>
                              </div>
                            </>
                          ) : (
                            <>
                              <div className="shrink-0">
                                <StatusBadge state={item.run.state} />
                              </div>
                              <div className="min-w-0 flex-1">
                                <p className="truncate text-sm text-white">
                                  {formatRunName(item.run)}
                                </p>
                                <p className="text-[11px] text-muted">
                                  {item.run.trigger} · {formatActivityTime(item.run.createdAt)}
                                </p>
                              </div>
                            </>
                          )}
                        </DropdownMenu.Item>
                      ))
                    )}
                  </div>
                </DropdownMenu.Content>
              </DropdownMenu.Portal>
            </DropdownMenu.Root>

            {isSaveableTab && (
              <button
                onClick={handleHeaderSave}
                disabled={!hasDirtyChanges || savingTab === agentTab}
                className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-colors ${
                  hasDirtyChanges
                    ? 'bg-warning/20 text-warning border border-warning/50 hover:bg-warning/30'
                    : 'bg-surface text-muted border border-edge'
                }`}
                title={agentTab === 'config' ? 'Save configuration' : 'Save pulse'}
              >
                <Save size={12} />
                {savingTab === agentTab ? 'Saving...' : 'Save'}
                {hasDirtyChanges && <span className="w-1.5 h-1.5 rounded-full bg-warning" />}
              </button>
            )}

            <button
              onClick={() => setShowRunDialog(true)}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover text-white text-xs font-medium transition-colors"
            >
              <Play size={12} /> Run Agent
            </button>
          </div>
        </div>

        <div className="flex gap-1 px-6">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setAgentTab(tab.id)}
              className={`flex items-center gap-1.5 px-3 py-2 text-xs font-medium rounded-t-lg border-b-2 transition-colors ${
                agentTab === tab.id
                  ? 'border-accent text-white bg-accent/10'
                  : 'border-transparent text-muted hover:text-white'
              }`}
            >
              <tab.icon size={13} />
              {tab.label}
            </button>
          ))}
        </div>
      </div>

      <div className="flex-1 overflow-hidden">
        {agentTab === 'chat' && (
          <ChatWorkspace
            agentId={agentId}
            controller={chatController}
            sidebarWidth={320}
            sidebarCollapsible
            sessionsCollapsed={chatSidebarCollapsed}
            onToggleSessions={() => setChatSidebarCollapsed((current) => !current)}
            emptyStateCopy="Select or start a chat"
          />
        )}
        {agentTab === 'workspace' && <WorkspaceTab agentId={agentId} />}
        <div className={agentTab === 'config' ? 'h-full' : 'hidden'}>
          <ConfigTab
            agentId={agentId}
            agentName={agent.name}
            onDirtyChange={(dirty) => handleDirtyChange('config', dirty)}
            ref={configSaveRef}
          />
        </div>
        <div className={agentTab === 'schedules' ? 'h-full' : 'hidden'}>
          <SchedulesTab
            agentId={agentId}
            onDirtyChange={(dirty) => handleDirtyChange('schedules', dirty)}
            ref={schedulesSaveRef}
          />
        </div>
        {agentTab === 'skills' && <SkillsTab agentId={agentId} />}
        {agentTab === 'bus' && <BusTab agentId={agentId} />}
      </div>

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

function HeaderStatChip({
  label,
  value,
  accent = false,
}: {
  label: string;
  value: string;
  accent?: boolean;
}) {
  return (
    <div
      className={`rounded-full border px-2.5 py-1 text-[11px] ${
        accent
          ? 'border-accent/50 bg-accent/10 text-accent-hover'
          : 'border-edge bg-surface text-secondary'
      }`}
    >
      <span className="text-muted">{label}</span> <span className="text-white">{value}</span>
    </div>
  );
}

function formatActivityTime(timestamp: string) {
  return new Date(timestamp).toLocaleString([], {
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  });
}

function formatRunName(run: RunSummary) {
  return run.taskName.includes('Pulse') ? 'Pulse' : run.taskName;
}

function formatSessionType(session: ChatSession) {
  switch (session.sessionType) {
    case 'pulse':
      return 'Pulse chat';
    case 'bus_message':
      return 'Bus message';
    case 'sub_agent':
      return 'Sub-agent';
    default:
      return 'Chat';
  }
}

function SessionTypeIcon({ sessionType }: { sessionType: ChatSession['sessionType'] }) {
  if (sessionType === 'pulse') {
    return <Zap size={14} className="text-warning" />;
  }

  if (sessionType === 'sub_agent') {
    return <GitBranch size={14} className="text-emerald-400" />;
  }

  if (sessionType === 'bus_message') {
    return <Activity size={14} className="text-blue-400" />;
  }

  return <MessageSquare size={14} className="text-accent-hover" />;
}
