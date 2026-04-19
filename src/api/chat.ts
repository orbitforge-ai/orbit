import { invoke } from '@tauri-apps/api/core';
import {
  ChatSession,
  ChatMessage,
  ChatModelOverride,
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
    sessionTypes?: string[],
    projectId?: string
  ): Promise<ChatSession[]> =>
    invoke('list_chat_sessions', { agentId, includeArchived, sessionTypes, projectId }),

  createSession: (
    agentId: string,
    title?: string,
    sessionType?: string,
    projectId?: string
  ): Promise<ChatSession> =>
    invoke('create_chat_session', { agentId, title, sessionType, projectId }),

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

  sendMessage: (
    sessionId: string,
    content: ContentBlock[],
    modelOverride?: ChatModelOverride
  ): Promise<SendChatMessageResponse> =>
    invoke('send_chat_message', {
      sessionId,
      content: JSON.stringify(content),
      modelOverride,
    }),

  respondToUserQuestion: (requestId: string, response: string): Promise<void> =>
    invoke('respond_to_user_question', { requestId, response }),

  getSessionExecution: (sessionId: string): Promise<SessionExecutionStatus> =>
    invoke('get_session_execution', { sessionId }),

  getSessionMeta: (
    sessionId: string
  ): Promise<{
    sessionId: string;
    agentId: string;
    projectId: string | null;
    projectName: string | null;
  }> => invoke('get_chat_session_meta', { sessionId }),

  cancelAgentSession: (sessionId: string): Promise<void> =>
    invoke('cancel_agent_session', { sessionId }),

  getContextUsage: (sessionId: string, modelOverride?: ChatModelOverride): Promise<ContextUsage> =>
    invoke('get_context_usage', { sessionId, modelOverride }),

  compactSession: (sessionId: string): Promise<void> =>
    invoke('compact_chat_session', { sessionId }),

  getReactions: (sessionId: string): Promise<MessageReaction[]> =>
    invoke('get_message_reactions', { sessionId }),
};
