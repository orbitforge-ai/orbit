import { create } from "zustand";

type Screen =
  | "dashboard"
  | "tasks"
  | "history"
  | "agents"
  | "schedules"
  | "task-builder"
  | "schedule-builder";

interface UiStore {
  screen: Screen;
  selectedRunId: string | null;
  selectedTaskId: string | null;
  logPanelOpen: boolean;

  navigate: (screen: Screen) => void;
  selectRun: (id: string | null) => void;
  selectTask: (id: string | null) => void;
  setLogPanelOpen: (open: boolean) => void;
}

export const useUiStore = create<UiStore>((set) => ({
  screen: "dashboard",
  selectedRunId: null,
  selectedTaskId: null,
  logPanelOpen: false,

  navigate: (screen) => set({ screen }),
  selectRun: (id) => set({ selectedRunId: id, logPanelOpen: id !== null }),
  selectTask: (id) => set({ selectedTaskId: id }),
  setLogPanelOpen: (open) => set({ logPanelOpen: open }),
}));
