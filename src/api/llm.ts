import { invoke } from "@tauri-apps/api/core";

export const llmApi = {
  setApiKey: (provider: string, key: string): Promise<void> =>
    invoke("set_api_key", { provider, key }),

  hasApiKey: (provider: string): Promise<boolean> =>
    invoke("has_api_key", { provider }),

  deleteApiKey: (provider: string): Promise<void> =>
    invoke("delete_api_key", { provider }),

  triggerAgentLoop: (agentId: string, goal: string): Promise<string> =>
    invoke("trigger_agent_loop", { agentId, goal }),
};
