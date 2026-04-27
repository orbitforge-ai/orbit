import { listen } from '../api/transport';
import {
  AgentCreatedPayload,
  AgentUpdatedPayload,
  AgentDeletedPayload,
  AgentConfigChangedPayload,
} from '../types';

export function onAgentCreated(handler: (payload: AgentCreatedPayload) => void) {
  return listen<AgentCreatedPayload>('agent:created', (e) => handler(e.payload));
}

export function onAgentUpdated(handler: (payload: AgentUpdatedPayload) => void) {
  return listen<AgentUpdatedPayload>('agent:updated', (e) => handler(e.payload));
}

export function onAgentDeleted(handler: (payload: AgentDeletedPayload) => void) {
  return listen<AgentDeletedPayload>('agent:deleted', (e) => handler(e.payload));
}

export function onAgentConfigChanged(handler: (payload: AgentConfigChangedPayload) => void) {
  return listen<AgentConfigChangedPayload>('agent:config_changed', (e) => handler(e.payload));
}
