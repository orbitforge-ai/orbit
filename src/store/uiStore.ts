import { create } from 'zustand';

type Screen =
  | 'dashboard'
  | 'tasks'
  | 'history'
  | 'agents'
  | 'schedules'
  | 'task-builder'
  | 'schedule-builder'
  | 'task-edit';

function getPersistedScreen(): Screen {
  try {
    const saved = localStorage.getItem('orbit:lastScreen');
    if (saved === 'chat') return 'agents';
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

type AgentTab = 'chat' | 'workspace' | 'config' | 'skills' | 'schedules' | 'bus';

interface UiStore {
  screen: Screen;
  selectedRunId: string | null;
  selectedTaskId: string | null;
  editingTaskId: string | null;
  selectedAgentId: string | null;
  pendingChatSessionId: string | null;
  logPanelOpen: boolean;
  agentTab: AgentTab;

  navigate: (screen: Screen) => void;
  selectRun: (id: string | null) => void;
  selectTask: (id: string | null) => void;
  editTask: (id: string) => void;
  selectAgent: (id: string) => void;
  openAgentChat: (agentId: string, sessionId?: string | null) => void;
  clearPendingChatSession: () => void;
  setLogPanelOpen: (open: boolean) => void;
  setAgentTab: (tab: AgentTab) => void;
}

export const useUiStore = create<UiStore>((set) => ({
  screen: getPersistedScreen(),
  selectedRunId: null,
  selectedTaskId: null,
  editingTaskId: null,
  selectedAgentId: getPersistedAgentId(),
  pendingChatSessionId: null,
  logPanelOpen: false,
  agentTab: 'chat' as AgentTab,

  navigate: (screen) => {
    try {
      localStorage.setItem('orbit:lastScreen', screen);
    } catch {}
    set({ screen });
  },
  selectRun: (id) => set({ selectedRunId: id, logPanelOpen: id !== null }),
  selectTask: (id) => set({ selectedTaskId: id }),
  editTask: (id) => set({ editingTaskId: id, screen: 'task-edit' }),
  selectAgent: (id) => {
    try {
      localStorage.setItem('orbit:lastScreen', 'agents');
      localStorage.setItem('orbit:lastAgentId', id);
    } catch {}
    set({ selectedAgentId: id, screen: 'agents' });
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
      agentTab: 'chat',
    });
  },
  clearPendingChatSession: () => set({ pendingChatSessionId: null }),
  setLogPanelOpen: (open) => set({ logPanelOpen: open }),
  setAgentTab: (tab) => set({ agentTab: tab }),
}));
