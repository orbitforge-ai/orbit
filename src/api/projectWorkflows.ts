import { invoke } from '@tauri-apps/api/core';
import {
  CreateProjectWorkflow,
  ProjectWorkflow,
  UpdateProjectWorkflow,
} from '../types';

export const projectWorkflowsApi = {
  list: (projectId: string): Promise<ProjectWorkflow[]> =>
    invoke('list_project_workflows', { projectId }),

  get: (id: string): Promise<ProjectWorkflow> =>
    invoke('get_project_workflow', { id }),

  create: (payload: CreateProjectWorkflow): Promise<ProjectWorkflow> =>
    invoke('create_project_workflow', { payload }),

  update: (id: string, payload: UpdateProjectWorkflow): Promise<ProjectWorkflow> =>
    invoke('update_project_workflow', { id, payload }),

  delete: (id: string): Promise<void> =>
    invoke('delete_project_workflow', { id }),

  setEnabled: (id: string, enabled: boolean): Promise<ProjectWorkflow> =>
    invoke('set_project_workflow_enabled', { id, enabled }),
};
