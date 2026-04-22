import { invoke } from '@tauri-apps/api/core';
import {
  Agent,
  CreateProjectBoard,
  DeleteProjectBoard,
  FileEntry,
  Project,
  ProjectAgent,
  ProjectBoard,
  ProjectBoardColumn,
  ProjectSummary,
  UpdateProjectBoard,
} from '../types';

export interface ProjectAgentWithMeta {
  agent: Agent;
  isDefault: boolean;
}

export const projectsApi = {
  list: (): Promise<ProjectSummary[]> => invoke('list_projects'),

  get: (id: string): Promise<Project> => invoke('get_project', { id }),

  create: (payload: {
    name: string;
    description?: string;
    boardPresetId?: 'starter' | 'lean';
  }): Promise<Project> =>
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

  listBoards: (projectId: string): Promise<ProjectBoard[]> =>
    invoke('list_project_boards', { projectId }),

  createBoard: (payload: CreateProjectBoard): Promise<ProjectBoard> =>
    invoke('create_project_board', { payload }),

  updateBoard: (id: string, payload: UpdateProjectBoard): Promise<ProjectBoard> =>
    invoke('update_project_board', { id, payload }),

  deleteBoard: (id: string, payload: DeleteProjectBoard = {}): Promise<void> =>
    invoke('delete_project_board', { id, payload }),

  listBoardColumns: (projectId: string, boardId?: string): Promise<ProjectBoardColumn[]> =>
    invoke('list_project_board_columns', { projectId, boardId }),

  createBoardColumn: (payload: {
    projectId: string;
    boardId?: string;
    name: string;
    role?: ProjectBoardColumn['role'];
    isDefault?: boolean;
    position?: number;
  }): Promise<ProjectBoardColumn> => invoke('create_project_board_column', { payload }),

  updateBoardColumn: (
    id: string,
    payload: {
      name?: string;
      role?: ProjectBoardColumn['role'];
      isDefault?: boolean;
      position?: number;
      expectedRevision?: string;
    },
  ): Promise<ProjectBoardColumn> => invoke('update_project_board_column', { id, payload }),

  deleteBoardColumn: (
    id: string,
    payload: {
      destinationColumnId?: string;
      force?: boolean;
      expectedRevision?: string;
    } = {},
  ): Promise<void> => invoke('delete_project_board_column', { id, payload }),

  reorderBoardColumns: (
    projectId: string,
    orderedIds: string[],
    boardId?: string,
    expectedRevision?: string,
  ): Promise<ProjectBoardColumn[]> =>
    invoke('reorder_project_board_columns', {
      projectId,
      payload: { orderedIds, boardId, expectedRevision },
    }),

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
