import { listen } from '@tauri-apps/api/event';
import {
  WorkflowRunCreatedPayload,
  WorkflowRunStepPayload,
  WorkflowRunUpdatedPayload,
} from '../types';

export function onWorkflowRunCreated(handler: (payload: WorkflowRunCreatedPayload) => void) {
  return listen<WorkflowRunCreatedPayload>('workflow_run:created', (event) => {
    handler(event.payload);
  });
}

export function onWorkflowRunUpdated(handler: (payload: WorkflowRunUpdatedPayload) => void) {
  return listen<WorkflowRunUpdatedPayload>('workflow_run:updated', (event) => {
    handler(event.payload);
  });
}

export function onWorkflowRunStep(handler: (payload: WorkflowRunStepPayload) => void) {
  return listen<WorkflowRunStepPayload>('workflow_run:step', (event) => {
    handler(event.payload);
  });
}
