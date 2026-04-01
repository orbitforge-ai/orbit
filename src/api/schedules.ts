import { invoke } from '@tauri-apps/api/core';
import { CreateSchedule, RecurringConfig, Schedule } from '../types';

export const schedulesApi = {
  list: (): Promise<Schedule[]> => invoke('list_schedules'),

  listForTask: (taskId: string): Promise<Schedule[]> =>
    invoke('get_schedules_for_task', { taskId }),

  create: (payload: CreateSchedule): Promise<Schedule> => invoke('create_schedule', { payload }),

  toggle: (id: string, enabled: boolean): Promise<void> =>
    invoke('toggle_schedule', { id, enabled }),

  delete: (id: string): Promise<void> => invoke('delete_schedule', { id }),

  previewNextRuns: (config: RecurringConfig, n = 5): Promise<string[]> =>
    invoke('preview_next_runs', { config, n }),
};
