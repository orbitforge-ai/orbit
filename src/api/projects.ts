import { invoke } from '@tauri-apps/api/core';
import {
  Agent,
  FileEntry,
  Project,
  ProjectAgent,
  ProjectBoardColumn,
  ProjectSummary,
} from '../types';

export interface ProjectAgentWithMeta {
  agent: Agent;
  isDefault: boolean;
}

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

  listAgentsWithMeta: (projectId: string): Promise<ProjectAgentWithMeta[]> =>
    invoke('list_project_agents_with_meta', { projectId }),

  listAgentProjects: (agentId: string): Promise<Project[]> =>
    invoke('list_agent_projects', { agentId }),

  addAgent: (projectId: string, agentId: string, isDefault = false): Promise<ProjectAgent> =>
    invoke('add_agent_to_project', { projectId, agentId, isDefault }),

  removeAgent: (projectId: string, agentId: string): Promise<void> =>
    invoke('remove_agent_from_project', { projectId, agentId }),

  listBoardColumns: (projectId: string): Promise<ProjectBoardColumn[]> =>
    invoke('list_project_board_columns', { projectId }),

  createBoardColumn: (payload: {
    projectId: string;
    name: string;
    status: ProjectBoardColumn['status'];
    position?: number;
  }): Promise<ProjectBoardColumn> => invoke('create_project_board_column', { payload }),

  updateBoardColumn: (
    id: string,
    payload: Partial<Pick<ProjectBoardColumn, 'name' | 'status' | 'position'>>,
  ): Promise<ProjectBoardColumn> => invoke('update_project_board_column', { id, payload }),

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

  createDir: (projectId: string, path: string): Promise<void> =>
    invoke('create_project_workspace_dir', { projectId, path }),

  renameEntry: (projectId: string, from: string, to: string): Promise<void> =>
    invoke('rename_project_workspace_entry', { projectId, from, to }),
};
