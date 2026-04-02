import { useState, useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
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
  X,
} from 'lucide-react';
import { cn } from '../lib/cn';
import { useUiStore } from '../store/uiStore';
import { agentsApi } from '../api/agents';
import { projectsApi } from '../api/projects';
import { workspaceApi } from '../api/workspace';
import { Agent, Project } from '../types';
import { usePermissionStore } from '../store/permissionStore';
import { onPermissionRequest, onPermissionCancelled } from '../events/permissionEvents';
import { SyncIndicator } from './SyncIndicator';
import { resolveRole } from '../lib/agentRoles';
import { ROLE_ICON_MAP } from '../screens/AgentInspector/RoleSelector';

const GLOBAL_NAV = [
  { id: 'dashboard' as const, label: 'Dashboard', icon: LayoutDashboard },
  { id: 'tasks' as const, label: 'Tasks', icon: ListChecks },
  { id: 'history' as const, label: 'Run History', icon: History },
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
  const pendingCount = usePermissionStore((s) => s.pendingCount);

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

  const { data: agents = [] } = useQuery<Agent[]>({
    queryKey: ['agents'],
    queryFn: agentsApi.list,
    refetchInterval: 10_000,
  });

  const { data: projects = [] } = useQuery<Project[]>({
    queryKey: ['projects'],
    queryFn: projectsApi.list,
    refetchInterval: 15_000,
  });

  const { data: agentRoleIds = {} } = useQuery<Record<string, string>>({
    queryKey: ['agent-role-ids'],
    queryFn: workspaceApi.listAgentRoleIds,
    staleTime: 30_000,
  });

  const selectedProject = projects.find((p) => p.id === selectedProjectId) ?? null;

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
            <span className="flex-1 text-left">Projects</span>
            <ChevronRight
              size={14}
              className={cn('transition-transform text-muted', projectsOpen && 'rotate-90')}
            />
          </button>

          {projectsOpen && (
            <div className="ml-3 mt-0.5 space-y-0.5 border-l border-edge pl-2">
              {/* Active project sub-nav */}
              {selectedProject ? (
                <>
                  <div className="flex items-center gap-1 px-2 py-1">
                    <span className="flex-1 truncate text-xs font-semibold text-accent-hover">
                      {selectedProject.name}
                    </span>
                    <button
                      onClick={() => selectProject(null)}
                      className="rounded p-0.5 text-muted hover:text-white hover:bg-surface transition-colors"
                      title="Exit project"
                    >
                      <X size={10} />
                    </button>
                  </div>
                  {PROJECT_TABS.map(({ id, label, icon: Icon }) => (
                    <button
                      key={id}
                      onClick={() => {
                        setProjectTab(id);
                        navigate('projects');
                      }}
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
                  <div className="my-1 border-t border-edge" />
                </>
              ) : null}

              {/* Project list */}
              {projects.map((project) => (
                <button
                  key={project.id}
                  onClick={() => selectProject(project.id)}
                  className={cn(
                    'w-full flex items-center gap-2 px-2.5 py-1.5 rounded-md text-xs font-medium transition-colors truncate',
                    selectedProjectId === project.id && screen === 'projects'
                      ? 'text-accent-hover'
                      : 'text-secondary hover:bg-surface hover:text-white'
                  )}
                >
                  <FolderOpen size={12} className="shrink-0" />
                  <span className="truncate">{project.name}</span>
                </button>
              ))}

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

        <div className="my-1 border-t border-edge" />

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
            <span className="flex-1 text-left">Agents</span>
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
                        className={cn('shrink-0', isSelected ? 'text-accent-hover' : role.color)}
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

      <div className="px-2 pt-2 border-t border-edge">
        <SyncIndicator />
      </div>

      <div className="p-3">
        <button
          onClick={() => navigate('task-builder')}
          className="w-full flex items-center justify-center gap-2 px-3 py-2 rounded-lg bg-accent hover:bg-accent-hover text-white text-sm font-medium transition-colors"
        >
          <Plus size={14} />
          New Task
        </button>
      </div>
    </aside>
  );
}
