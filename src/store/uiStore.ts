import { create } from "zustand";

type Screen =
  | "dashboard"
  | "tasks"
  | "history"
  | "agents"
  | "schedules"
  | "sessions"
  | "task-builder"
  | "schedule-builder"
  | "task-edit";

function getPersistedScreen(): Screen {
  try {
    const saved = localStorage.getItem("orbit:lastScreen");
    if (saved) return saved as Screen;
  } catch {}
  return "dashboard";
}

type AgentTab = "overview" | "workspace" | "config";

interface UiStore {
  screen: Screen;
  selectedRunId: string | null;
  selectedTaskId: string | null;
  editingTaskId: string | null;
  logPanelOpen: boolean;
  agentTab: AgentTab;

  navigate: (screen: Screen) => void;
  selectRun: (id: string | null) => void;
  selectTask: (id: string | null) => void;
  editTask: (id: string) => void;
  setLogPanelOpen: (open: boolean) => void;
  setAgentTab: (tab: AgentTab) => void;
}

export const useUiStore = create<UiStore>((set) => ({
  screen: getPersistedScreen(),
  selectedRunId: null,
  selectedTaskId: null,
  editingTaskId: null,
  logPanelOpen: false,
  agentTab: "overview" as AgentTab,

  navigate: (screen) => {
    try { localStorage.setItem("orbit:lastScreen", screen); } catch {}
    set({ screen });
  },
  selectRun: (id) => set({ selectedRunId: id, logPanelOpen: id !== null }),
  selectTask: (id) => set({ selectedTaskId: id }),
  editTask: (id) => set({ editingTaskId: id, screen: "task-edit" }),
  setLogPanelOpen: (open) => set({ logPanelOpen: open }),
  setAgentTab: (tab) => set({ agentTab: tab }),
}));
