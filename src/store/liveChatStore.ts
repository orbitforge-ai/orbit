import { create } from 'zustand';
import {
  AgentContentBlockPayload,
  AgentIterationPayload,
  ContentBlock,
  MessageReactionPayload,
  PermissionRequestPayload,
  UserQuestionPayload,
} from '../types';
import { DisplayMessage } from '../components/chat/types';
import {
  appendPreviewBlock,
  appendTextDelta as appendPreviewTextDelta,
  applyContentBlock,
  applyToolResult,
  contentBlocksToDisplay,
  createAssistantPreview,
  createEmptyPreviewState,
  finalizePreviewMessage,
  StreamPreviewState,
} from './streaming/streamReducer';

interface LiveChatStream extends StreamPreviewState {
  streamId: string;
  sessionId: string;
  turnId: string;
  isStreaming: boolean;
  pendingUserMessage: DisplayMessage | null;
  userMessageDbId: string | null;
  completedAt: number | null;
}

interface LiveChatStore {
  chatStreams: Record<string, LiveChatStream>;
  startChatStream: (streamId: string, sessionId: string, content: ContentBlock[]) => {
    turnId: string;
    userMessageId: string;
  };
  setUserMessageDbId: (streamId: string, localMessageId: string, dbId: string) => void;
  appendTextDelta: (streamId: string, delta: string) => void;
  addContentBlock: (streamId: string, payload: AgentContentBlockPayload) => void;
  addToolResult: (streamId: string, toolUseId: string, content: string, isError: boolean) => void;
  addPermissionPrompt: (streamId: string, payload: PermissionRequestPayload) => void;
  addUserQuestionPrompt: (streamId: string, payload: UserQuestionPayload) => void;
  addReaction: (streamId: string, payload: MessageReactionPayload) => void;
  handleIteration: (streamId: string, payload: AgentIterationPayload) => void;
  clearChatStream: (streamId: string) => void;
}

type LiveChatStoreState = {
  chatStreams: Record<string, LiveChatStream>;
};

let localMessageCounter = 0;

function nextLocalMessageId(prefix: string) {
  localMessageCounter += 1;
  return `${prefix}-${localMessageCounter}`;
}

function ensureStream(state: LiveChatStoreState, streamId: string): LiveChatStream {
  const existing = state.chatStreams[streamId];
  if (existing) return existing;

  const sessionId = streamId.startsWith('chat:') ? streamId.slice(5) : streamId;
  return {
    streamId,
    sessionId,
    turnId: nextLocalMessageId('chat-turn'),
    isStreaming: true,
    pendingUserMessage: null,
    userMessageDbId: null,
    completedAt: null,
    ...createEmptyPreviewState(),
  };
}

function finalizeCurrentPreview(stream: LiveChatStream, keepStreaming: boolean): LiveChatStream {
  if (!stream.previewMessage) return stream;
  return {
    ...stream,
    previewMessage: finalizePreviewMessage(stream.previewMessage, keepStreaming),
  };
}

export const useLiveChatStore = create<LiveChatStore>((set) => ({
  chatStreams: {},

  startChatStream: (streamId, sessionId, content) => {
    const userMessageId = nextLocalMessageId('chat-user');
    const turnId = nextLocalMessageId('chat-turn');

    set((state) => ({
      chatStreams: {
        ...state.chatStreams,
        [streamId]: {
          streamId,
          sessionId,
          turnId,
          isStreaming: true,
          pendingUserMessage: {
            id: userMessageId,
            role: 'user',
            blocks: contentBlocksToDisplay(content),
            isStreaming: false,
            timestamp: new Date().toISOString(),
          },
          userMessageDbId: null,
          completedAt: null,
          ...createEmptyPreviewState(),
          previewMessage: createAssistantPreview(() => nextLocalMessageId('chat-preview')),
        },
      },
    }));

    return { turnId, userMessageId };
  },

  setUserMessageDbId: (streamId, localMessageId, dbId) =>
    set((state) => {
      const stream = state.chatStreams[streamId];
      if (!stream || !stream.pendingUserMessage || stream.pendingUserMessage.id !== localMessageId) {
        return state;
      }

      return {
        chatStreams: {
          ...state.chatStreams,
          [streamId]: {
            ...stream,
            userMessageDbId: dbId,
            pendingUserMessage: {
              ...stream.pendingUserMessage,
              dbId,
            },
          },
        },
      };
    }),

  appendTextDelta: (streamId, delta) =>
    set((state) => {
      const stream = ensureStream(state, streamId);
      const previewState = appendPreviewTextDelta(stream, delta, () =>
        nextLocalMessageId('chat-preview')
      );

      return {
        chatStreams: {
          ...state.chatStreams,
          [streamId]: {
            ...stream,
            ...previewState,
            isStreaming: true,
            completedAt: null,
          },
        },
      };
    }),

  addContentBlock: (streamId, payload) =>
    set((state) => {
      const stream = ensureStream(state, streamId);
      const previewState = applyContentBlock(stream, payload, () =>
        nextLocalMessageId('chat-preview')
      );

      return {
        chatStreams: {
          ...state.chatStreams,
          [streamId]: {
            ...stream,
            ...previewState,
            isStreaming: true,
            completedAt: null,
          },
        },
      };
    }),

  addToolResult: (streamId, toolUseId, content, isError) =>
    set((state) => {
      const stream = state.chatStreams[streamId];
      if (!stream) return state;

      const applied = applyToolResult([], stream, toolUseId, content, isError);
      return {
        chatStreams: {
          ...state.chatStreams,
          [streamId]: {
            ...stream,
            ...applied.state,
          },
        },
      };
    }),

  addPermissionPrompt: (streamId, payload) =>
    set((state) => {
      const stream = ensureStream(state, streamId);
      const previewState = appendPreviewBlock(
        stream,
        {
          kind: 'permission_prompt',
          requestId: payload.requestId,
          toolName: payload.toolName,
          toolInput: payload.toolInput,
          riskLevel: payload.riskLevel,
          riskDescription: payload.riskDescription,
          suggestedPattern: payload.suggestedPattern,
        },
        () => nextLocalMessageId('chat-preview')
      );

      return {
        chatStreams: {
          ...state.chatStreams,
          [streamId]: {
            ...stream,
            ...previewState,
            isStreaming: true,
            completedAt: null,
          },
        },
      };
    }),

  addUserQuestionPrompt: (streamId, payload) =>
    set((state) => {
      const stream = ensureStream(state, streamId);
      const previewState = appendPreviewBlock(
        stream,
        {
          kind: 'user_question_prompt',
          requestId: payload.requestId,
          question: payload.question,
          choices: payload.choices ?? undefined,
          allowCustom: payload.allowCustom,
          multiSelect: payload.multiSelect,
          context: payload.context ?? undefined,
        },
        () => nextLocalMessageId('chat-preview')
      );

      return {
        chatStreams: {
          ...state.chatStreams,
          [streamId]: {
            ...stream,
            ...previewState,
            isStreaming: true,
            completedAt: null,
          },
        },
      };
    }),

  addReaction: (streamId, payload) =>
    set((state) => {
      const stream = state.chatStreams[streamId];
      if (!stream?.pendingUserMessage || stream.pendingUserMessage.dbId !== payload.messageId) {
        return state;
      }

      return {
        chatStreams: {
          ...state.chatStreams,
          [streamId]: {
            ...stream,
            pendingUserMessage: {
              ...stream.pendingUserMessage,
              reactions: [
                ...(stream.pendingUserMessage.reactions ?? []),
                { id: payload.reactionId, emoji: payload.emoji, isNew: true },
              ],
            },
          },
        },
      };
    }),

  handleIteration: (streamId, payload) =>
    set((state) => {
      const stream = state.chatStreams[streamId];
      if (!stream) return state;

      if (payload.action === 'llm_call') {
        return {
          chatStreams: {
            ...state.chatStreams,
            [streamId]: {
              ...finalizeCurrentPreview(stream, true),
              completedAt: null,
            },
          },
        };
      }

      if (payload.action === 'finished') {
        return {
          chatStreams: {
            ...state.chatStreams,
            [streamId]: {
              ...finalizeCurrentPreview(stream, false),
              isStreaming: false,
              completedAt: Date.now(),
            },
          },
        };
      }

      return state;
    }),

  clearChatStream: (streamId) =>
    set((state) => {
      const { [streamId]: _removed, ...rest } = state.chatStreams;
      return { chatStreams: rest };
    }),
}));
