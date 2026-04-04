import { useState, useEffect } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import {
  LayoutDashboard,
  ListChecks,
  History,
  Bot,
  Clock,
  MessageSquare,
  Plus,
  ChevronRight,
  Shield,
  FolderOpen,
  HardDrive,
  Users,
  Settings,
} from 'lucide-react';
import { cn } from '../lib/cn';
import { useUiStore } from '../store/uiStore';
import { agentsApi } from '../api/agents';
import { projectsApi } from '../api/projects';
import { workspaceApi } from '../api/workspace';
import { Agent, Project } from '../types';
import { usePermissionStore } from '../store/permissionStore';
import { onPermissionRequest, onPermissionCancelled } from '../events/permissionEvents';
import { onAgentCreated, onAgentUpdated, onAgentDeleted, onAgentConfigChanged } from '../events/agentEvents';
import { SyncIndicator } from './SyncIndicator';
import { resolveRole } from '../lib/agentRoles';
import { ROLE_ICON_MAP } from '../screens/AgentInspector/RoleSelector';

const GLOBAL_NAV = [
  { id: 'dashboard' as const, label: 'Dashboard', icon: LayoutDashboard },
  { id: 'tasks' as const, label: 'All Tasks', icon: ListChecks },
  { id: 'history' as const, label: 'All History', icon: History },
  { id: 'schedules' as const, label: 'Schedules', icon: Clock },
];

const PROJECT_TABS = [
  { id: 'workspace' as const, label: 'Workspace', icon: HardDrive },
  { id: 'agents' as const, label: 'Agents', icon: Users },
  { id: 'tasks' as const, label: 'Tasks', icon: ListChecks },
  { id: 'history' as const, label: 'History', icon: History },
];

export function Sidebar() {
  const {
    screen,
    selectedAgentId,
    selectedProjectId,
    projectTab,
    navigate,
    selectAgent,
    selectProject,
    setProjectTab,
    openAgentChat,
  } = useUiStore();
  const [agentsOpen, setAgentsOpen] = useState(screen === 'agents');
  const [projectsOpen, setProjectsOpen] = useState(true);
  const [expandedProjectIds, setExpandedProjectIds] = useState<Set<string>>(
    () => selectedProjectId ? new Set([selectedProjectId]) : new Set()
  );
  const toggleProjectExpanded = (projectId: string) => {
    setExpandedProjectIds(prev => {
      const next = new Set(prev);
      if (next.has(projectId)) { next.delete(projectId); } else { next.add(projectId); }
      return next;
    });
  };
  const pendingCount = usePermissionStore((s) => s.pendingCount);
  const queryClient = useQueryClient();

  useEffect(() => {
    const unsubs: Promise<() => void>[] = [];
    unsubs.push(
      onPermissionRequest((payload) => {
        usePermissionStore.getState().addRequest(payload);
      })
    );
    unsubs.push(
      onPermissionCancelled((payload) => {
        usePermissionStore.getState().removeRequest(payload.requestId);
      })
    );
    return () => {
      unsubs.forEach((p) => p.then((fn) => fn()).catch(() => {}));
    };
  }, []);

  useEffect(() => {
    const unsubs: Promise<() => void>[] = [];

    unsubs.push(
      onAgentCreated((p) => {
        queryClient.setQueryData<Agent[]>(['agents'], (old = []) => [...old, p.agent]);
        if (p.roleId) {
          queryClient.setQueryData<Record<string, string>>(['agent-role-ids'], (old = {}) => ({
            ...old,
            [p.agent.id]: p.roleId!,
          }));
        }
      })
    );

    unsubs.push(
      onAgentUpdated((p) => {
        queryClient.setQueryData<Agent[]>(['agents'], (old = []) =>
          old.map((a) => (a.id === p.agent.id ? p.agent : a))
        );
      })
    );

    unsubs.push(
      onAgentDeleted((p) => {
        queryClient.setQueryData<Agent[]>(['agents'], (old = []) =>
          old.filter((a) => a.id !== p.agentId)
        );
        queryClient.setQueryData<Record<string, string>>(['agent-role-ids'], (old = {}) => {
          const next = { ...old };
          delete next[p.agentId];
          return next;
        });
      })
    );

    unsubs.push(
      onAgentConfigChanged((p) => {
        queryClient.setQueryData<Record<string, string>>(['agent-role-ids'], (old = {}) => {
          const next = { ...old };
          if (p.roleId) {
            next[p.agentId] = p.roleId;
          } else {
            delete next[p.agentId];
          }
          return next;
        });
      })
    );

    return () => {
      unsubs.forEach((p) => p.then((fn) => fn()).catch(() => {}));
    };
  }, [queryClient]);

  const { data: agents = [] } = useQuery<Agent[]>({
    queryKey: ['agents'],
    queryFn: agentsApi.list,
  });

  const { data: projects = [] } = useQuery<Project[]>({
    queryKey: ['projects'],
    queryFn: projectsApi.list,
    refetchInterval: 15_000,
  });

  const { data: agentRoleIds = {} } = useQuery<Record<string, string>>({
    queryKey: ['agent-role-ids'],
    queryFn: workspaceApi.listAgentRoleIds,
  });

  return (
    <aside className="w-[220px] flex-shrink-0 flex flex-col border-r border-edge bg-panel h-full">
      <nav className="flex-1 px-2 py-3 space-y-0.5 overflow-y-auto">

        {/* ── Projects section ── */}
        <div>
          <button
            onClick={() => setProjectsOpen(!projectsOpen)}
            className={cn(
              'w-full flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm font-medium transition-colors',
              screen === 'projects'
                ? 'bg-accent/15 text-accent-hover'
                : 'text-secondary hover:bg-surface hover:text-white'
            )}
          >
            <FolderOpen size={16} />
            <span className="flex-1 text-left text-xs font-semibold uppercase tracking-wide">Projects</span>
            <ChevronRight
              size={14}
              className={cn('transition-transform text-muted', projectsOpen && 'rotate-90')}
            />
          </button>

          {projectsOpen && (
            <div className="ml-3 mt-0.5 space-y-0.5 border-l border-edge pl-2">
              {/* Project list (accordion) */}
              {projects.map((project) => {
                const isExpanded = expandedProjectIds.has(project.id);
                const isSelected = selectedProjectId === project.id && screen === 'projects';
                return (
                  <div key={project.id}>
                    <div className={cn(
                      'group flex items-center rounded-md transition-colors',
                      isSelected
                        ? 'text-accent-hover'
                        : 'text-secondary hover:bg-surface hover:text-white'
                    )}>
                      <button
                        onClick={() => {
                          selectProject(project.id);
                          setExpandedProjectIds(prev => new Set([...prev, project.id]));
                        }}
                        className="flex flex-1 min-w-0 items-center gap-2 px-2.5 py-1.5 text-xs font-medium truncate"
                      >
                        <FolderOpen size={12} className="shrink-0" />
                        <span className="truncate text-left">{project.name}</span>
                      </button>
                      <button
                        onClick={() => toggleProjectExpanded(project.id)}
                        className="pr-2 py-1.5 text-muted hover:text-white transition-colors"
                        aria-label={isExpanded ? 'Collapse' : 'Expand'}
                      >
                        <ChevronRight size={10} className={cn('transition-transform', isExpanded && 'rotate-90')} />
                      </button>
                    </div>
                    {isExpanded && (
                      <div className="ml-3 mt-0.5 mb-1 space-y-0.5 border-l border-edge pl-2">
                        {PROJECT_TABS.map(({ id, label, icon: Icon }) => (
                          <button
                            key={id}
                            onClick={() => { setProjectTab(id); navigate('projects'); }}
                            className={cn(
                              'w-full flex items-center gap-2 px-2.5 py-1.5 rounded-md text-xs font-medium transition-colors',
                              screen === 'projects' && projectTab === id
                                ? 'bg-accent/10 text-accent-hover'
                                : 'text-secondary hover:bg-surface hover:text-white'
                            )}
                          >
                            <Icon size={12} />
                            {label}
                          </button>
                        ))}
                      </div>
                    )}
                  </div>
                );
              })}

              {/* New Project */}
              <button
                onClick={() => {
                  useUiStore.setState({ screen: 'projects', selectedProjectId: null });
                  setTimeout(() => {
                    window.dispatchEvent(new CustomEvent('orbit:new-project'));
                  }, 50);
                }}
                className="w-full flex items-center gap-2 px-2.5 py-1.5 rounded-md text-xs font-medium text-muted hover:text-accent-hover hover:bg-accent/10 transition-colors"
              >
                <Plus size={12} />
                <span>New Project</span>
              </button>
            </div>
          )}
        </div>

        {/* ── Global nav ── */}
        {GLOBAL_NAV.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            onClick={() => navigate(id)}
            className={cn(
              'w-full flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm font-medium transition-colors',
              screen === id
                ? 'bg-accent/15 text-accent-hover'
                : 'text-secondary hover:bg-surface hover:text-white'
            )}
          >
            <Icon size={16} />
            {label}
          </button>
        ))}

        {/* ── Agents collapsible ── */}
        <div>
          <button
            onClick={() => setAgentsOpen(!agentsOpen)}
            className={cn(
              'w-full flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm font-medium transition-colors',
              screen === 'agents'
                ? 'bg-accent/15 text-accent-hover'
                : 'text-secondary hover:bg-surface hover:text-white'
            )}
          >
            <Bot size={16} />
            <span className="flex-1 text-left text-xs font-semibold uppercase tracking-wide">Agents</span>
            {pendingCount > 0 && (
              <span className="flex items-center gap-1 px-1.5 py-0.5 rounded-full bg-amber-500/20 text-amber-400 text-[10px] font-medium">
                <Shield size={8} />
                {pendingCount}
              </span>
            )}
            <ChevronRight
              size={14}
              className={cn('transition-transform text-muted', agentsOpen && 'rotate-90')}
            />
          </button>

          {agentsOpen && (
            <div className="ml-3 mt-0.5 space-y-0.5 border-l border-edge pl-2">
              {agents.map((agent) => {
                const roleId = agentRoleIds[agent.id];
                const role = resolveRole(roleId);
                const RoleIcon = ROLE_ICON_MAP[role.icon] ?? Bot;
                const isSelected = screen === 'agents' && selectedAgentId === agent.id;
                return (
                  <div
                    key={agent.id}
                    className={cn(
                      'group flex items-center gap-1 rounded-md transition-colors',
                      isSelected
                        ? 'bg-accent/10 text-accent-hover'
                        : 'text-secondary hover:bg-surface hover:text-white'
                    )}
                  >
                    <button
                      onClick={() => selectAgent(agent.id)}
                      className="flex min-w-0 flex-1 items-center gap-2 px-2.5 py-1.5 text-xs font-medium truncate"
                    >
                      <RoleIcon
                        size={12}
                        className={cn('shrink-0', role.color)}
                      />
                      <span className="truncate">{agent.name}</span>
                    </button>
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        openAgentChat(agent.id);
                      }}
                      className="mr-1 rounded p-1 text-muted opacity-0 transition-opacity hover:bg-background/60 hover:text-white group-hover:opacity-100"
                      title={`Open ${agent.name} chat`}
                      aria-label={`Open ${agent.name} chat`}
                    >
                      <MessageSquare size={12} />
                    </button>
                  </div>
                );
              })}

              <button
                onClick={() => selectAgent('__new__')}
                className="w-full flex items-center gap-2 px-2.5 py-1.5 rounded-md text-xs font-medium text-muted hover:text-accent-hover hover:bg-accent/10 transition-colors"
              >
                <Plus size={12} />
                <span>New Agent</span>
              </button>
            </div>
          )}
        </div>
      </nav>

      <div className="flex items-center gap-1.5 border-t border-edge px-2 pt-2 min-w-0">
        <SyncIndicator />
        <button
          onClick={() => navigate('settings')}
          className={cn(
            'shrink-0 rounded-lg p-1.5 transition-colors',
            screen === 'settings'
              ? 'bg-accent/15 text-accent-hover'
              : 'text-muted hover:bg-surface hover:text-white'
          )}
          title="Settings"
          aria-label="Settings"
        >
          <Settings size={15} />
        </button>
      </div>

      <div className="px-2 pb-2">
        <DropdownMenu.Root>
          <DropdownMenu.Trigger asChild>
            <button className="w-full flex items-center justify-center gap-2 px-3 py-2 rounded-lg border border-edge text-secondary hover:bg-surface hover:text-white text-sm font-medium transition-colors">
              <Plus size={14} />
              New
            </button>
          </DropdownMenu.Trigger>
          <DropdownMenu.Portal>
            <DropdownMenu.Content
              side="top"
              align="center"
              sideOffset={6}
              className="z-50 w-48 rounded-xl border border-edge bg-surface p-1.5 shadow-xl"
            >
              <DropdownMenu.Item
                onSelect={() => navigate('task-builder')}
                className="flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm text-secondary outline-none cursor-pointer data-[highlighted]:bg-accent/10 data-[highlighted]:text-white"
              >
                <ListChecks size={14} />
                New Task
              </DropdownMenu.Item>
              <DropdownMenu.Item
                onSelect={() => selectAgent('__new__')}
                className="flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm text-secondary outline-none cursor-pointer data-[highlighted]:bg-accent/10 data-[highlighted]:text-white"
              >
                <Bot size={14} />
                New Agent
              </DropdownMenu.Item>
              <DropdownMenu.Item
                onSelect={() => {
                  useUiStore.setState({ screen: 'projects', selectedProjectId: null });
                  setTimeout(() => {
                    window.dispatchEvent(new CustomEvent('orbit:new-project'));
                  }, 50);
                }}
                className="flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm text-secondary outline-none cursor-pointer data-[highlighted]:bg-accent/10 data-[highlighted]:text-white"
              >
                <FolderOpen size={14} />
                New Project
              </DropdownMenu.Item>
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>
      </div>
    </aside>
  );
}
