import { invoke } from "@tauri-apps/api/core";
import { ChatSession, ChatMessage, ContentBlock, ContextUsage, PaginatedChatMessages } from "../types";

export const chatApi = {
  listSessions: (agentId: string, includeArchived?: boolean): Promise<ChatSession[]> =>
    invoke("list_chat_sessions", { agentId, includeArchived }),

  createSession: (agentId: string, title?: string): Promise<ChatSession> =>
    invoke("create_chat_session", { agentId, title }),

  renameSession: (sessionId: string, title: string): Promise<void> =>
    invoke("rename_chat_session", { sessionId, title }),

  archiveSession: (sessionId: string): Promise<void> =>
    invoke("archive_chat_session", { sessionId }),

  unarchiveSession: (sessionId: string): Promise<void> =>
    invoke("unarchive_chat_session", { sessionId }),

  deleteSession: (sessionId: string): Promise<void> =>
    invoke("delete_chat_session", { sessionId }),

  getMessages: (sessionId: string): Promise<ChatMessage[]> =>
    invoke<PaginatedChatMessages>("get_chat_messages", { sessionId })
      .then(res => res.messages),

  getMessagesPaginated: (sessionId: string, limit: number, offset: number): Promise<PaginatedChatMessages> =>
    invoke("get_chat_messages", { sessionId, limit, offset }),

  sendMessage: (sessionId: string, content: ContentBlock[]): Promise<string> =>
    invoke("send_chat_message", { sessionId, content: JSON.stringify(content) }),

  getContextUsage: (sessionId: string): Promise<ContextUsage> =>
    invoke("get_context_usage", { sessionId }),

  compactSession: (sessionId: string): Promise<void> =>
    invoke("compact_chat_session", { sessionId }),
};
