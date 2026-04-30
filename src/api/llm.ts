import { invoke } from './transport';

export type ProviderStatus = {
  kind: 'api_key' | 'cli';
  ready: boolean;
  binary_path: string | null;
  message: string | null;
};

export type ModelOption = {
  label: string;
  value: string;
};

export const llmApi = {
  setApiKey: (provider: string, key: string): Promise<void> =>
    invoke('set_api_key', { provider, key }),

  hasApiKey: (provider: string): Promise<boolean> => invoke('has_api_key', { provider }),

  deleteApiKey: (provider: string): Promise<void> => invoke('delete_api_key', { provider }),

  getProviderStatus: (provider: string): Promise<ProviderStatus> =>
    invoke('get_provider_status', { provider }),

  listVercelGatewayModels: (): Promise<ModelOption[]> => invoke('list_vercel_gateway_models'),

  triggerAgentLoop: (agentId: string, goal: string): Promise<string> =>
    invoke('trigger_agent_loop', { agentId, goal }),
};
