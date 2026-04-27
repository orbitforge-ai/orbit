import { invoke } from './transport';
import { Agent, CreateAgent, UpdateAgent } from '../types';

export const agentsApi = {
  list: (): Promise<Agent[]> => invoke('list_agents'),

  create: (payload: CreateAgent): Promise<Agent> => invoke('create_agent', { payload }),

  update: (id: string, payload: UpdateAgent): Promise<Agent> =>
    invoke('update_agent', { id, payload }),

  delete: (id: string): Promise<void> => invoke('delete_agent', { id }),

  cancelRun: (runId: string): Promise<void> => invoke('cancel_run', { runId }),
};
