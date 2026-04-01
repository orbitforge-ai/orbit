import { invoke } from '@tauri-apps/api/core';
import { CreateTask, Task } from '../types';

export const tasksApi = {
  list: (): Promise<Task[]> => invoke('list_tasks'),

  get: (id: string): Promise<Task> => invoke('get_task', { id }),

  create: (payload: CreateTask): Promise<Task> => invoke('create_task', { payload }),

  update: (id: string, payload: Partial<Task>): Promise<Task> =>
    invoke('update_task', { id, payload }),

  delete: (id: string): Promise<void> => invoke('delete_task', { id }),

  trigger: (taskId: string): Promise<string> => invoke('trigger_task', { taskId }),
};
