import { invoke } from '@tauri-apps/api/core';
import {
  ChatSession,
  ChatMessage,
  ContentBlock,
  ContextUsage,
  PaginatedChatMessages,
  SessionExecutionStatus,
  MessageReaction,
  SendChatMessageResponse,
} from '../types';

export const chatApi = {
  listSessions: (
    agentId: string,
    includeArchived?: boolean,
    sessionTypes?: string[]
  ): Promise<ChatSession[]> =>
    invoke('list_chat_sessions', { agentId, includeArchived, sessionTypes }),

  createSession: (agentId: string, title?: string, sessionType?: string): Promise<ChatSession> =>
    invoke('create_chat_session', { agentId, title, sessionType }),

  renameSession: (sessionId: string, title: string): Promise<void> =>
    invoke('rename_chat_session', { sessionId, title }),

  archiveSession: (sessionId: string): Promise<void> =>
    invoke('archive_chat_session', { sessionId }),

  unarchiveSession: (sessionId: string): Promise<void> =>
    invoke('unarchive_chat_session', { sessionId }),

  deleteSession: (sessionId: string): Promise<void> => invoke('delete_chat_session', { sessionId }),

  getMessages: (sessionId: string): Promise<ChatMessage[]> =>
    invoke<PaginatedChatMessages>('get_chat_messages', { sessionId }).then((res) => res.messages),

  getMessagesPaginated: (
    sessionId: string,
    limit: number,
    offset: number
  ): Promise<PaginatedChatMessages> => invoke('get_chat_messages', { sessionId, limit, offset }),

  sendMessage: (sessionId: string, content: ContentBlock[]): Promise<SendChatMessageResponse> =>
    invoke('send_chat_message', { sessionId, content: JSON.stringify(content) }),

  getSessionExecution: (sessionId: string): Promise<SessionExecutionStatus> =>
    invoke('get_session_execution', { sessionId }),

  cancelAgentSession: (sessionId: string): Promise<void> =>
    invoke('cancel_agent_session', { sessionId }),

  getContextUsage: (sessionId: string): Promise<ContextUsage> =>
    invoke('get_context_usage', { sessionId }),

  compactSession: (sessionId: string): Promise<void> =>
    invoke('compact_chat_session', { sessionId }),

  getReactions: (sessionId: string): Promise<MessageReaction[]> =>
    invoke('get_message_reactions', { sessionId }),
};
