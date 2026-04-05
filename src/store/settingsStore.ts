import { create } from 'zustand';

const SHOW_AGENT_THOUGHTS_KEY = 'orbit:showAgentThoughts';

function getPersistedShowAgentThoughts(): boolean {
  try {
    return localStorage.getItem(SHOW_AGENT_THOUGHTS_KEY) === 'true';
  } catch {
    return false;
  }
}

interface SettingsStore {
  showAgentThoughts: boolean;
  setShowAgentThoughts: (value: boolean) => void;
}

export const useSettingsStore = create<SettingsStore>((set) => ({
  showAgentThoughts: getPersistedShowAgentThoughts(),
  setShowAgentThoughts: (value) => {
    try {
      localStorage.setItem(SHOW_AGENT_THOUGHTS_KEY, String(value));
    } catch {}
    set({ showAgentThoughts: value });
  },
}));
