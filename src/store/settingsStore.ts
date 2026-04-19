import { create } from 'zustand';
import { globalSettingsApi } from '../api/globalSettings';
import {
  AgentDefaults,
  ChannelConfig,
  ChatDisplaySettings,
  DeveloperSettings,
  GlobalSettings,
  PermissionRule,
} from '../types';

// Legacy localStorage keys — imported one-time into the global settings file.
const SHOW_AGENT_THOUGHTS_KEY = 'orbit:showAgentThoughts';
const SHOW_VERBOSE_TOOL_DETAILS_KEY = 'orbit:showVerboseToolDetails';
const LEGACY_IMPORT_DONE_KEY = 'orbit:chatDisplayMigratedToGlobal';

function defaultSettings(): GlobalSettings {
  return {
    version: 1,
    chatDisplay: {
      showAgentThoughts: false,
      showVerboseToolDetails: false,
    },
    agentDefaults: {
      allowedTools: [],
      permissionMode: 'normal',
      permissionRules: [],
      webSearchProvider: 'brave',
    },
    channels: [],
    developer: {
      pluginDevMode: false,
    },
  };
}

function readLegacyChatDisplay(): Partial<ChatDisplaySettings> | null {
  try {
    if (localStorage.getItem(LEGACY_IMPORT_DONE_KEY) === 'true') return null;
    const thoughts = localStorage.getItem(SHOW_AGENT_THOUGHTS_KEY);
    const verbose = localStorage.getItem(SHOW_VERBOSE_TOOL_DETAILS_KEY);
    if (thoughts === null && verbose === null) return null;
    const imported: Partial<ChatDisplaySettings> = {};
    if (thoughts !== null) imported.showAgentThoughts = thoughts === 'true';
    if (verbose !== null) imported.showVerboseToolDetails = verbose === 'true';
    return imported;
  } catch {
    return null;
  }
}

function markLegacyImportDone() {
  try {
    localStorage.setItem(LEGACY_IMPORT_DONE_KEY, 'true');
    localStorage.removeItem(SHOW_AGENT_THOUGHTS_KEY);
    localStorage.removeItem(SHOW_VERBOSE_TOOL_DETAILS_KEY);
  } catch {}
}

interface SettingsStore {
  settings: GlobalSettings;
  loaded: boolean;
  loading: boolean;
  error: string | null;

  // Mirrored top-level fields so existing selector-based callers
  // (`s.showAgentThoughts`, `s.showVerboseToolDetails`) stay reactive
  // without migrating every consumer.
  showAgentThoughts: boolean;
  showVerboseToolDetails: boolean;

  hydrate: () => Promise<void>;
  saveSettings: (next: GlobalSettings) => Promise<void>;

  setShowAgentThoughts: (value: boolean) => Promise<void>;
  setShowVerboseToolDetails: (value: boolean) => Promise<void>;

  updateChatDisplay: (patch: Partial<ChatDisplaySettings>) => Promise<void>;
  updateAgentDefaults: (patch: Partial<AgentDefaults>) => Promise<void>;
  updateDeveloper: (patch: Partial<DeveloperSettings>) => Promise<void>;

  upsertChannel: (channel: ChannelConfig) => Promise<void>;
  removeChannel: (channelId: string) => Promise<void>;

  upsertPermissionRule: (rule: PermissionRule) => Promise<void>;
  removePermissionRule: (ruleId: string) => Promise<void>;
}

function syncedFromSettings(settings: GlobalSettings) {
  return {
    settings,
    showAgentThoughts: settings.chatDisplay.showAgentThoughts,
    showVerboseToolDetails: settings.chatDisplay.showVerboseToolDetails,
  };
}

export const useSettingsStore = create<SettingsStore>((set, get) => ({
  settings: defaultSettings(),
  showAgentThoughts: false,
  showVerboseToolDetails: false,
  loaded: false,
  loading: false,
  error: null,

  hydrate: async () => {
    if (get().loading) return;
    set({ loading: true, error: null });
    try {
      const remote = await globalSettingsApi.get();
      const legacy = readLegacyChatDisplay();
      let merged = remote;
      if (legacy) {
        // Legacy values win only if the remote is still at its built-in
        // defaults — we don't want to overwrite settings the user has
        // already touched through the new UI.
        const remoteDisplay = remote.chatDisplay;
        const remoteIsDefault =
          remoteDisplay.showAgentThoughts === false &&
          remoteDisplay.showVerboseToolDetails === false;
        if (remoteIsDefault) {
          merged = {
            ...remote,
            chatDisplay: { ...remoteDisplay, ...legacy },
          };
          try {
            merged = await globalSettingsApi.update(merged);
          } catch (err) {
            console.warn('Failed to persist migrated chat-display settings', err);
          }
        }
        markLegacyImportDone();
      }
      set({
        ...syncedFromSettings(merged),
        loaded: true,
        loading: false,
        error: null,
      });
    } catch (err) {
      console.error('Failed to load global settings', err);
      set({
        loading: false,
        error: err instanceof Error ? err.message : String(err),
      });
    }
  },

  saveSettings: async (next) => {
    const previous = get().settings;
    set(syncedFromSettings(next));
    try {
      const persisted = await globalSettingsApi.update(next);
      set(syncedFromSettings(persisted));
    } catch (err) {
      // Roll back optimistic update on failure.
      set({
        ...syncedFromSettings(previous),
        error: err instanceof Error ? err.message : String(err),
      });
      throw err;
    }
  },

  setShowAgentThoughts: async (value) => {
    await get().updateChatDisplay({ showAgentThoughts: value });
  },

  setShowVerboseToolDetails: async (value) => {
    await get().updateChatDisplay({ showVerboseToolDetails: value });
  },

  updateChatDisplay: async (patch) => {
    const current = get().settings;
    await get().saveSettings({
      ...current,
      chatDisplay: { ...current.chatDisplay, ...patch },
    });
  },

  updateAgentDefaults: async (patch) => {
    const current = get().settings;
    await get().saveSettings({
      ...current,
      agentDefaults: { ...current.agentDefaults, ...patch },
    });
  },

  updateDeveloper: async (patch) => {
    const current = get().settings;
    await get().saveSettings({
      ...current,
      developer: { ...current.developer, ...patch },
    });
  },

  upsertChannel: async (channel) => {
    const current = get().settings;
    const next = current.channels.filter((c) => c.id !== channel.id);
    next.push(channel);
    await get().saveSettings({ ...current, channels: next });
  },

  removeChannel: async (channelId) => {
    const current = get().settings;
    await get().saveSettings({
      ...current,
      channels: current.channels.filter((c) => c.id !== channelId),
    });
  },

  upsertPermissionRule: async (rule) => {
    const current = get().settings;
    const filtered = current.agentDefaults.permissionRules.filter(
      (r) => !(r.tool === rule.tool && r.pattern === rule.pattern),
    );
    filtered.push(rule);
    await get().saveSettings({
      ...current,
      agentDefaults: { ...current.agentDefaults, permissionRules: filtered },
    });
  },

  removePermissionRule: async (ruleId) => {
    const current = get().settings;
    await get().saveSettings({
      ...current,
      agentDefaults: {
        ...current.agentDefaults,
        permissionRules: current.agentDefaults.permissionRules.filter(
          (r) => r.id !== ruleId,
        ),
      },
    });
  },
}));

// Kick off the initial load as soon as the module is imported. Failures are
// swallowed here — the store surfaces them via the `error` field.
void useSettingsStore.getState().hydrate();
