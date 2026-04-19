import { invoke } from '@tauri-apps/api/core';
import { WorkflowRun, WorkflowRunWithSteps } from '../types';

export const workflowRunsApi = {
  start: (
    workflowId: string,
    triggerData?: Record<string, unknown>,
  ): Promise<WorkflowRun> =>
    invoke('start_workflow_run', {
      workflowId,
      triggerData: triggerData ?? null,
    }),

  list: (workflowId: string, limit?: number): Promise<WorkflowRun[]> =>
    invoke('list_workflow_runs', {
      workflowId,
      limit: limit ?? null,
    }),

  get: (runId: string): Promise<WorkflowRunWithSteps> =>
    invoke('get_workflow_run', { runId }),

  cancel: (runId: string): Promise<void> =>
    invoke('cancel_workflow_run', { runId }),
};
