import { invoke } from '@tauri-apps/api/core';
import { FileEntry, AgentWorkspaceConfig } from '../types';

export const workspaceApi = {
  initWorkspace: (agentId: string): Promise<void> => invoke('init_agent_workspace', { agentId }),

  getWorkspacePath: (agentId: string): Promise<string> =>
    invoke('get_workspace_path', { agentId }),

  listFiles: (agentId: string, path?: string): Promise<FileEntry[]> =>
    invoke('list_workspace_files', { agentId, path: path ?? null }),

  readFile: (agentId: string, path: string): Promise<string> =>
    invoke('read_workspace_file', { agentId, path }),

  writeFile: (agentId: string, path: string, content: string): Promise<void> =>
    invoke('write_workspace_file', { agentId, path, content }),

  deleteFile: (agentId: string, path: string): Promise<void> =>
    invoke('delete_workspace_file', { agentId, path }),

  createDir: (agentId: string, path: string): Promise<void> =>
    invoke('create_workspace_dir', { agentId, path }),

  renameEntry: (agentId: string, from: string, to: string): Promise<void> =>
    invoke('rename_workspace_entry', { agentId, from, to }),

  getConfig: (agentId: string): Promise<AgentWorkspaceConfig> =>
    invoke('get_agent_config', { agentId }),

  updateConfig: (agentId: string, config: AgentWorkspaceConfig): Promise<void> =>
    invoke('update_agent_config', { agentId, config }),

  updateSystemPrompt: (agentId: string, content: string): Promise<void> =>
    invoke('update_system_prompt', { agentId, content }),

  listAgentRoleIds: (): Promise<Record<string, string>> => invoke('list_agent_role_ids'),
};
