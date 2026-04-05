import { listen } from '@tauri-apps/api/event';
import {
  RunLogChunkPayload,
  RunStateChangedPayload,
  AgentLlmChunkPayload,
  AgentIterationPayload,
  AgentContentBlockPayload,
  AgentToolResultPayload,
  ChatContextUpdatePayload,
  CompactionStatusPayload,
  BusMessageSentPayload,
  SubAgentsSpawnedPayload,
  MessageReactionPayload,
} from '../types';

export function onRunLogChunk(handler: (payload: RunLogChunkPayload) => void) {
  return listen<RunLogChunkPayload>('run:log_chunk', (event) => {
    handler(event.payload);
  });
}

export function onRunStateChanged(handler: (payload: RunStateChangedPayload) => void) {
  return listen<RunStateChangedPayload>('run:state_changed', (event) => {
    handler(event.payload);
  });
}

export function onAgentLlmChunk(handler: (payload: AgentLlmChunkPayload) => void) {
  return listen<AgentLlmChunkPayload>('agent:llm_chunk', (event) => {
    handler(event.payload);
  });
}

export function onAgentIteration(handler: (payload: AgentIterationPayload) => void) {
  return listen<AgentIterationPayload>('agent:iteration', (event) => {
    handler(event.payload);
  });
}

export function onAgentContentBlock(handler: (payload: AgentContentBlockPayload) => void) {
  return listen<AgentContentBlockPayload>('agent:content_block', (event) => {
    handler(event.payload);
  });
}

export function onAgentToolResult(handler: (payload: AgentToolResultPayload) => void) {
  return listen<AgentToolResultPayload>('agent:tool_result', (event) => {
    handler(event.payload);
  });
}

export function onChatContextUpdate(handler: (payload: ChatContextUpdatePayload) => void) {
  return listen<ChatContextUpdatePayload>('chat:context_update', (event) => {
    handler(event.payload);
  });
}

export function onSubAgentsSpawned(handler: (payload: SubAgentsSpawnedPayload) => void) {
  return listen<SubAgentsSpawnedPayload>('agent:sub_agents_spawned', (event) => {
    handler(event.payload);
  });
}

export function onCompactionStatus(handler: (payload: CompactionStatusPayload) => void) {
  return listen<CompactionStatusPayload>('compaction:status', (event) => {
    handler(event.payload);
  });
}

export function onBusMessageSent(handler: (payload: BusMessageSentPayload) => void) {
  return listen<BusMessageSentPayload>('bus:message_sent', (event) => {
    handler(event.payload);
  });
}

export function onMessageReaction(handler: (payload: MessageReactionPayload) => void) {
  return listen<MessageReactionPayload>('message:reaction', (event) => {
    handler(event.payload);
  });
}
