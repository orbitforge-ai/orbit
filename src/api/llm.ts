import { invoke } from '@tauri-apps/api/core';

export type ProviderStatus = {
  kind: 'api_key' | 'cli';
  ready: boolean;
  binary_path: string | null;
  message: string | null;
};

export const llmApi = {
  setApiKey: (provider: string, key: string): Promise<void> =>
    invoke('set_api_key', { provider, key }),

  hasApiKey: (provider: string): Promise<boolean> => invoke('has_api_key', { provider }),

  deleteApiKey: (provider: string): Promise<void> => invoke('delete_api_key', { provider }),

  getProviderStatus: (provider: string): Promise<ProviderStatus> =>
    invoke('get_provider_status', { provider }),

  triggerAgentLoop: (agentId: string, goal: string): Promise<string> =>
    invoke('trigger_agent_loop', { agentId, goal }),
};
