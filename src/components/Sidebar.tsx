import { useState, useEffect } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import {
  DndContext,
  DragOverlay,
  useDraggable,
  useDroppable,
  type DragEndEvent,
  type DragStartEvent,
} from '@dnd-kit/core';
import {
  agentDraggableId,
  parseAgentDraggableId,
  parseProjectDroppableId,
  projectDroppableId,
  useAgentDndSensors,
  useAssignAgentToProject,
} from './dnd/agentDnd';
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
  Brain,
  HardDrive,
  Users,
  Settings,
  KanbanSquare,
  Workflow,
} from 'lucide-react';
import { cn } from '../lib/cn';
import { useUiStore } from '../store/uiStore';
import { agentsApi } from '../api/agents';
import { projectsApi } from '../api/projects';
import { workspaceApi } from '../api/workspace';
import { Agent, PermissionRequestPayload, ProjectSummary } from '../types';
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
  { id: 'memory' as const, label: 'Memory', icon: Brain },
];

const PROJECT_TABS = [
  { id: 'workspace' as const, label: 'Workspace', icon: HardDrive },
  { id: 'agents' as const, label: 'Agents', icon: Users },
  { id: 'chat' as const, label: 'Chat', icon: MessageSquare },
  { id: 'board' as const, label: 'Board', icon: KanbanSquare },
  { id: 'scheduled' as const, label: 'Scheduled', icon: ListChecks },
  { id: 'workflows' as const, label: 'Workflows', icon: Workflow },
  { id: 'history' as const, label: 'History', icon: History },
];

export function Sidebar() {
  const {
    screen,
    settingsOpen,
    selectedAgentId,
    selectedProjectId,
    projectTab,
    navigate,
    openSettings,
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
  const pendingRequestMap = usePermissionStore((s) => s.pending);
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

  const { data: projects = [] } = useQuery<ProjectSummary[]>({
    queryKey: ['projects'],
    queryFn: projectsApi.list,
    refetchInterval: 15_000,
  });

  const { data: agentRoleIds = {} } = useQuery<Record<string, string>>({
    queryKey: ['agent-role-ids'],
    queryFn: workspaceApi.listAgentRoleIds,
  });

  // ─── Drag-and-drop: agent → project assignment ──────────────────────────────
  const dndSensors = useAgentDndSensors();
  const assignAgent = useAssignAgentToProject();
  const [draggingAgentId, setDraggingAgentId] = useState<string | null>(null);

  const handleDragStart = (event: DragStartEvent) => {
    setDraggingAgentId(parseAgentDraggableId(event.active.id));
  };

  const handleDragEnd = (event: DragEndEvent) => {
    setDraggingAgentId(null);
    const agentId = parseAgentDraggableId(event.active.id);
    const projectId = parseProjectDroppableId(event.over?.id);
    if (!agentId || !projectId) return;
    assignAgent.mutate({ projectId, agentId });
  };

  const draggingAgent = draggingAgentId
    ? agents.find((a) => a.id === draggingAgentId) ?? null
    : null;

  const sortedPendingRequests = Object.values(pendingRequestMap).sort(
    (a, b) => new Date(a.timestamp).getTime() - new Date(b.timestamp).getTime()
  );
  const agentNameById = new Map(agents.map((agent) => [agent.id, agent.name]));
  const pendingByAgentId = sortedPendingRequests.reduce<
    Record<string, PermissionRequestPayload[]>
  >((acc, request) => {
    if (!acc[request.agentId]) {
      acc[request.agentId] = [];
    }
    acc[request.agentId].push(request);
    return acc;
  }, {});

  return (
    <DndContext sensors={dndSensors} onDragStart={handleDragStart} onDragEnd={handleDragEnd}>
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
                    <DroppableSidebarProject projectId={project.id}>
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
                      {project.agentCount > 0 && (
                        <span
                          className="shrink-0 px-1.5 py-0.5 rounded-full bg-surface text-muted text-[10px] font-medium tabular-nums"
                          title={`${project.agentCount} agent${project.agentCount === 1 ? '' : 's'} assigned`}
                        >
                          {project.agentCount}
                        </span>
                      )}
                      <button
                        onClick={() => toggleProjectExpanded(project.id)}
                        className="pr-2 py-1.5 text-muted hover:text-white transition-colors"
                        aria-label={isExpanded ? 'Collapse' : 'Expand'}
                      >
                        <ChevronRight size={10} className={cn('transition-transform', isExpanded && 'rotate-90')} />
                      </button>
                    </div>
                    </DroppableSidebarProject>
                    {isExpanded && (
                      <div className="ml-3 mt-0.5 mb-1 space-y-0.5 border-l border-edge pl-2">
                        {PROJECT_TABS.map(({ id, label, icon: Icon }) => (
                          <button
                            key={id}
                            onClick={() => {
                              selectProject(project.id);
                              setProjectTab(id);
                            }}
                            className={cn(
                              'w-full flex items-center gap-2 px-2.5 py-1.5 rounded-md text-xs font-medium transition-colors',
                              screen === 'projects' &&
                                selectedProjectId === project.id &&
                                projectTab === id
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
              {sortedPendingRequests.length > 0 && (
                <div className="mb-2 rounded-lg border border-amber-500/20 bg-amber-500/5 p-2">
                  <div className="mb-1.5 flex items-center gap-1.5 px-0.5 text-[10px] font-semibold uppercase tracking-wide text-amber-300">
                    <Shield size={10} />
                    Pending Approvals
                  </div>
                  <div className="space-y-1">
                    {sortedPendingRequests.slice(0, 3).map((request) => {
                      const agentName = agentNameById.get(request.agentId) ?? 'Unknown agent';
                      return (
                        <button
                          key={request.requestId}
                          onClick={() => openAgentChat(request.agentId, request.sessionId)}
                          className="w-full rounded-md border border-transparent px-2 py-1.5 text-left transition-colors hover:border-amber-500/20 hover:bg-background/60"
                        >
                          <div className="flex items-center justify-between gap-2">
                            <span className="truncate text-[11px] font-medium text-white">
                              {agentName}
                            </span>
                            <span className="shrink-0 text-[10px] text-amber-300">
                              {request.toolName}
                            </span>
                          </div>
                          <div className="truncate text-[10px] text-muted">
                            {request.riskDescription}
                          </div>
                        </button>
                      );
                    })}
                  </div>
                  {sortedPendingRequests.length > 3 && (
                    <div className="mt-1 px-0.5 text-[10px] text-muted">
                      +{sortedPendingRequests.length - 3} more pending approval
                      {sortedPendingRequests.length - 3 === 1 ? '' : 's'}
                    </div>
                  )}
                </div>
              )}

              {agents.map((agent) => {
                const roleId = agentRoleIds[agent.id];
                const role = resolveRole(roleId);
                const RoleIcon = ROLE_ICON_MAP[role.icon] ?? Bot;
                const isSelected = screen === 'agents' && selectedAgentId === agent.id;
                const pendingForAgent = pendingByAgentId[agent.id] ?? [];
                const firstPendingRequest = pendingForAgent[0];
                return (
                  <DraggableSidebarAgent key={agent.id} agentId={agent.id}>
                  <div
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
                    {firstPendingRequest && (
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          openAgentChat(agent.id, firstPendingRequest.sessionId);
                        }}
                        className="flex items-center gap-1 rounded px-1.5 py-1 text-amber-400 transition-colors hover:bg-amber-500/10 hover:text-amber-300"
                        title={`Review ${pendingForAgent.length} pending approval${pendingForAgent.length === 1 ? '' : 's'} for ${agent.name}`}
                        aria-label={`Review pending approvals for ${agent.name}`}
                      >
                        <Shield size={10} />
                        <span className="text-[10px] font-medium">{pendingForAgent.length}</span>
                      </button>
                    )}
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
                  </DraggableSidebarAgent>
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
          onClick={openSettings}
          className={cn(
            'shrink-0 rounded-lg p-1.5 transition-colors',
            settingsOpen
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
                  useUiStore.setState({
                    screen: 'projects',
                    selectedProjectId: null,
                    settingsOpen: false,
                  });
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
    <DragOverlay>
      {draggingAgent ? (
        <div className="pointer-events-none rounded-md border border-accent bg-surface px-2.5 py-1.5 text-xs font-medium text-white shadow-lg">
          {draggingAgent.name}
        </div>
      ) : null}
    </DragOverlay>
    </DndContext>
  );
}

// ─── Drag-and-drop wrappers ──────────────────────────────────────────────────

/**
 * Makes a sidebar agent row draggable. Renders children inside a div with
 * the dnd-kit listeners attached and a subtle cursor affordance.
 */
function DraggableSidebarAgent({
  agentId,
  children,
}: {
  agentId: string;
  children: React.ReactNode;
}) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id: agentDraggableId(agentId),
  });
  return (
    <div
      ref={setNodeRef}
      {...attributes}
      {...listeners}
      className={cn(
        'cursor-grab active:cursor-grabbing touch-none',
        isDragging && 'opacity-40'
      )}
    >
      {children}
    </div>
  );
}

/**
 * Makes a sidebar project row a drop target for agents. Highlights the row
 * while a compatible item is hovering it.
 */
function DroppableSidebarProject({
  projectId,
  children,
}: {
  projectId: string;
  children: React.ReactNode;
}) {
  const { setNodeRef, isOver } = useDroppable({
    id: projectDroppableId(projectId),
  });
  return (
    <div
      ref={setNodeRef}
      className={cn(
        'rounded-md transition-colors',
        isOver && 'bg-accent/20 ring-1 ring-accent'
      )}
    >
      {children}
    </div>
  );
}
