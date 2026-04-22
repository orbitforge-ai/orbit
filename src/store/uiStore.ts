import { create } from 'zustand';

type Screen =
  | 'dashboard'
  | 'tasks'
  | 'history'
  | 'agents'
  | 'schedules'
  | 'memory'
  | 'projects'
  | 'task-builder'
  | 'schedule-builder'
  | 'task-edit'
  | 'workflow-editor'
  | 'plugins'
  | 'plugin-entities'
  | 'settings';

function getPersistedScreen(): Screen {
  try {
    const saved = localStorage.getItem('orbit:lastScreen');
    if (saved === 'chat') return 'agents';
    if (saved === 'settings') return 'dashboard';
    if (saved) return saved as Screen;
  } catch {}
  return 'dashboard';
}

function getPersistedAgentId(): string | null {
  try {
    return localStorage.getItem('orbit:lastAgentId');
  } catch {}
  return null;
}

type AgentTab = 'chat' | 'workspace' | 'config' | 'skills' | 'schedules' | 'bus' | 'listen';
type ProjectTab =
  | 'workspace'
  | 'agents'
  | 'board'
  | 'chat'
  | 'scheduled'
  | 'workflows'
  | 'history';

function getPersistedProjectId(): string | null {
  try {
    return localStorage.getItem('orbit:lastProjectId');
  } catch {}
  return null;
}

function getPersistedSelectedBoardByProject(): Record<string, string> {
  try {
    const raw = localStorage.getItem('orbit:selectedBoardByProject');
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    if (parsed && typeof parsed === 'object') return parsed as Record<string, string>;
  } catch {}
  return {};
}

interface UiStore {
  screen: Screen;
  settingsOpen: boolean;
  selectedRunId: string | null;
  selectedTaskId: string | null;
  editingTaskId: string | null;
  selectedAgentId: string | null;
  selectedProjectId: string | null;
  selectedWorkflowId: string | null;
  pendingChatSessionId: string | null;
  logPanelOpen: boolean;
  agentTab: AgentTab;
  projectTab: ProjectTab;
  selectedBoardIdByProject: Record<string, string>;

  navigate: (screen: Screen) => void;
  openSettings: () => void;
  closeSettings: () => void;
  selectRun: (id: string | null) => void;
  selectTask: (id: string | null) => void;
  editTask: (id: string) => void;
  selectAgent: (id: string) => void;
  selectProject: (id: string | null) => void;
  openWorkflowEditor: (workflowId: string) => void;
  closeWorkflowEditor: () => void;
  openAgentChat: (agentId: string, sessionId?: string | null) => void;
  clearPendingChatSession: () => void;
  setLogPanelOpen: (open: boolean) => void;
  setAgentTab: (tab: AgentTab) => void;
  setProjectTab: (tab: ProjectTab) => void;
  setSelectedBoard: (projectId: string, boardId: string) => void;
}

export const useUiStore = create<UiStore>((set) => ({
  screen: getPersistedScreen(),
  settingsOpen: false,
  selectedRunId: null,
  selectedTaskId: null,
  editingTaskId: null,
  selectedAgentId: getPersistedAgentId(),
  selectedProjectId: getPersistedProjectId(),
  selectedWorkflowId: null,
  pendingChatSessionId: null,
  logPanelOpen: false,
  agentTab: 'chat' as AgentTab,
  projectTab: 'workspace' as ProjectTab,
  selectedBoardIdByProject: getPersistedSelectedBoardByProject(),

  navigate: (screen) => {
    if (screen === 'settings') {
      set({ settingsOpen: true });
      return;
    }
    try {
      localStorage.setItem('orbit:lastScreen', screen);
    } catch {}
    set({ screen, settingsOpen: false });
  },
  openSettings: () => set({ settingsOpen: true }),
  closeSettings: () => set({ settingsOpen: false }),
  selectRun: (id) => set({ selectedRunId: id, logPanelOpen: id !== null }),
  selectTask: (id) => set({ selectedTaskId: id }),
  editTask: (id) => set({ editingTaskId: id, screen: 'task-edit', settingsOpen: false }),
  selectAgent: (id) => {
    try {
      localStorage.setItem('orbit:lastScreen', 'agents');
      localStorage.setItem('orbit:lastAgentId', id);
    } catch {}
    set({ selectedAgentId: id, screen: 'agents', settingsOpen: false });
  },
  openAgentChat: (agentId, sessionId = null) => {
    try {
      localStorage.setItem('orbit:lastScreen', 'agents');
      localStorage.setItem('orbit:lastAgentId', agentId);
    } catch {}
    set({
      selectedAgentId: agentId,
      pendingChatSessionId: sessionId,
      screen: 'agents',
      settingsOpen: false,
      agentTab: 'chat',
    });
  },
  selectProject: (id) => {
    try {
      if (id) {
        localStorage.setItem('orbit:lastProjectId', id);
        localStorage.setItem('orbit:lastScreen', 'projects');
      } else {
        localStorage.removeItem('orbit:lastProjectId');
      }
    } catch {}
    set({ selectedProjectId: id, screen: 'projects', settingsOpen: false });
  },
  openWorkflowEditor: (workflowId) =>
    set({
      selectedWorkflowId: workflowId,
      screen: 'workflow-editor',
      settingsOpen: false,
    }),
  closeWorkflowEditor: () =>
    set({
      selectedWorkflowId: null,
      screen: 'projects',
      projectTab: 'workflows',
    }),
  clearPendingChatSession: () => set({ pendingChatSessionId: null }),
  setLogPanelOpen: (open) => set({ logPanelOpen: open }),
  setAgentTab: (tab) => set({ agentTab: tab }),
  setProjectTab: (tab) => set({ projectTab: tab }),
  setSelectedBoard: (projectId, boardId) =>
    set((state) => {
      const next = { ...state.selectedBoardIdByProject, [projectId]: boardId };
      try {
        localStorage.setItem('orbit:selectedBoardByProject', JSON.stringify(next));
      } catch {}
      return { selectedBoardIdByProject: next };
    }),
}));
