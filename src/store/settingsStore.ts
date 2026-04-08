import { create } from 'zustand';

const SHOW_AGENT_THOUGHTS_KEY = 'orbit:showAgentThoughts';
const SHOW_VERBOSE_TOOL_DETAILS_KEY = 'orbit:showVerboseToolDetails';

function getPersistedShowAgentThoughts(): boolean {
  try {
    return localStorage.getItem(SHOW_AGENT_THOUGHTS_KEY) === 'true';
  } catch {
    return false;
  }
}

function getPersistedShowVerboseToolDetails(): boolean {
  try {
    return localStorage.getItem(SHOW_VERBOSE_TOOL_DETAILS_KEY) === 'true';
  } catch {
    return false;
  }
}

interface SettingsStore {
  showAgentThoughts: boolean;
  showVerboseToolDetails: boolean;
  setShowAgentThoughts: (value: boolean) => void;
  setShowVerboseToolDetails: (value: boolean) => void;
}

export const useSettingsStore = create<SettingsStore>((set) => ({
  showAgentThoughts: getPersistedShowAgentThoughts(),
  showVerboseToolDetails: getPersistedShowVerboseToolDetails(),
  setShowAgentThoughts: (value) => {
    try {
      localStorage.setItem(SHOW_AGENT_THOUGHTS_KEY, String(value));
    } catch {}
    set({ showAgentThoughts: value });
  },
  setShowVerboseToolDetails: (value) => {
    try {
      localStorage.setItem(SHOW_VERBOSE_TOOL_DETAILS_KEY, String(value));
    } catch {}
    set({ showVerboseToolDetails: value });
  },
}));
