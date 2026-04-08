import { invoke } from '@tauri-apps/api/core';
import { Agent, FileEntry, Project, ProjectAgent, ProjectSummary } from '../types';

export const projectsApi = {
  list: (): Promise<ProjectSummary[]> => invoke('list_projects'),

  get: (id: string): Promise<Project> => invoke('get_project', { id }),

  create: (payload: { name: string; description?: string }): Promise<Project> =>
    invoke('create_project', { payload }),

  update: (id: string, payload: { name?: string; description?: string }): Promise<Project> =>
    invoke('update_project', { id, payload }),

  delete: (id: string): Promise<void> => invoke('delete_project', { id }),

  // Agent membership
  listAgents: (projectId: string): Promise<Agent[]> =>
    invoke('list_project_agents', { projectId }),

  listAgentProjects: (agentId: string): Promise<Project[]> =>
    invoke('list_agent_projects', { agentId }),

  addAgent: (projectId: string, agentId: string, isDefault = false): Promise<ProjectAgent> =>
    invoke('add_agent_to_project', { projectId, agentId, isDefault }),

  removeAgent: (projectId: string, agentId: string): Promise<void> =>
    invoke('remove_agent_from_project', { projectId, agentId }),

  // Workspace file operations
  getWorkspacePath: (projectId: string): Promise<string> =>
    invoke('get_project_workspace_path', { projectId }),

  listFiles: (projectId: string, path?: string): Promise<FileEntry[]> =>
    invoke('list_project_workspace_files', { projectId, path }),

  readFile: (projectId: string, path: string): Promise<string> =>
    invoke('read_project_workspace_file', { projectId, path }),

  writeFile: (projectId: string, path: string, content: string): Promise<void> =>
    invoke('write_project_workspace_file', { projectId, path, content }),

  deleteFile: (projectId: string, path: string): Promise<void> =>
    invoke('delete_project_workspace_file', { projectId, path }),
};
