import { create } from 'zustand';
import {
  AgentContentBlockPayload,
  AgentIterationPayload,
  ContentBlock,
  MessageReactionPayload,
  PermissionRequestPayload,
  UserQuestionPayload,
} from '../types';
import { DisplayBlock, DisplayMessage } from '../components/chat/types';

interface LiveChatStream {
  streamId: string;
  sessionId: string;
  isStreaming: boolean;
  displayMessages: DisplayMessage[];
  completedAt: number | null;
}

interface LiveChatStore {
  chatStreams: Record<string, LiveChatStream>;
  startChatStream: (
    streamId: string,
    sessionId: string,
    baseMessages: DisplayMessage[],
    content: ContentBlock[]
  ) => string;
  setUserMessageDbId: (streamId: string, localMessageId: string, dbId: string) => void;
  appendTextDelta: (streamId: string, delta: string) => void;
  addContentBlock: (streamId: string, payload: AgentContentBlockPayload) => void;
  addToolResult: (streamId: string, toolUseId: string, content: string, isError: boolean) => void;
  addPermissionPrompt: (streamId: string, payload: PermissionRequestPayload) => void;
  addUserQuestionPrompt: (streamId: string, payload: UserQuestionPayload) => void;
  addReaction: (streamId: string, payload: MessageReactionPayload) => void;
  handleIteration: (streamId: string, payload: AgentIterationPayload) => void;
  completeChatStream: (streamId: string) => void;
  clearChatStream: (streamId: string) => void;
}

let localMessageCounter = 0;
const cleanupTimeouts = new Map<string, ReturnType<typeof setTimeout>>();

function nextLocalMessageId(prefix: string) {
  localMessageCounter += 1;
  return `${prefix}-${localMessageCounter}`;
}

function contentBlocksToDisplay(content: ContentBlock[]): DisplayBlock[] {
  return content.map((block): DisplayBlock => {
    if (block.type === 'text') return { kind: 'text', text: block.text, isStreaming: false };
    if (block.type === 'image') {
      return { kind: 'image', mediaType: block.media_type, data: block.data };
    }
    return { kind: 'text', text: '[attachment]', isStreaming: false };
  });
}

function finalizeStreamingMessage(message: DisplayMessage): DisplayMessage {
  const blocks = [...message.blocks];
  const lastBlock = blocks[blocks.length - 1];
  if (lastBlock && lastBlock.kind === 'text' && lastBlock.isStreaming) {
    blocks[blocks.length - 1] = { ...lastBlock, isStreaming: false };
  }
  return { ...message, blocks, isStreaming: false };
}

function cloneDisplayMessage(message: DisplayMessage): DisplayMessage {
  return {
    ...message,
    blocks: message.blocks.map((block) => ({ ...block })),
    reactions: message.reactions?.map((reaction) => ({ ...reaction })),
  };
}

function ensureStream(
  state: LiveChatStoreState,
  streamId: string
): LiveChatStream {
  const existing = state.chatStreams[streamId];
  if (existing) return existing;

  const sessionId = streamId.startsWith('chat:') ? streamId.slice(5) : streamId;
  const created: LiveChatStream = {
    streamId,
    sessionId,
    isStreaming: true,
    displayMessages: [],
    completedAt: null,
  };
  return created;
}

function getOrCreateAssistantMessage(msgs: DisplayMessage[]): [DisplayMessage[], number] {
  const last = msgs[msgs.length - 1];
  if (last && last.role === 'assistant' && last.isStreaming) {
    return [msgs, msgs.length - 1];
  }

  const newMsg: DisplayMessage = {
    id: nextLocalMessageId('chat-stream'),
    role: 'assistant',
    blocks: [],
    isStreaming: true,
  };
  return [[...msgs, newMsg], msgs.length];
}

function hasPrimaryContent(message: DisplayMessage | undefined): boolean {
  if (!message) return false;
  return message.blocks.some((block) => block.kind !== 'thinking' && block.kind !== 'tool_call');
}

function maybeAppendCompletionMessage(
  messages: DisplayMessage[],
  finishSummary?: string | null
): DisplayMessage[] {
  const summary = finishSummary?.trim();
  if (!summary) return messages;

  const last = messages[messages.length - 1];
  if (last?.role === 'assistant' && !hasPrimaryContent(last)) {
    return [
      ...messages,
      {
        id: nextLocalMessageId('chat-complete'),
        role: 'assistant',
        blocks: [{ kind: 'text', text: summary, isStreaming: false }],
        isStreaming: false,
      },
    ];
  }

  return messages;
}

type LiveChatStoreState = {
  chatStreams: Record<string, LiveChatStream>;
};

function scheduleCleanup(streamId: string) {
  const existing = cleanupTimeouts.get(streamId);
  if (existing) clearTimeout(existing);
  const timeout = setTimeout(() => {
    useLiveChatStore.getState().clearChatStream(streamId);
  }, 30_000);
  cleanupTimeouts.set(streamId, timeout);
}

function cancelCleanup(streamId: string) {
  const existing = cleanupTimeouts.get(streamId);
  if (existing) {
    clearTimeout(existing);
    cleanupTimeouts.delete(streamId);
  }
}

export const useLiveChatStore = create<LiveChatStore>((set) => ({
  chatStreams: {},

  startChatStream: (streamId, sessionId, baseMessages, content) => {
    cancelCleanup(streamId);
    const userMessageId = nextLocalMessageId('chat-user');
    const assistantMessageId = nextLocalMessageId('chat-assistant');
    const nextMessages = [
      ...baseMessages.map(cloneDisplayMessage),
      {
        id: userMessageId,
        role: 'user' as const,
        blocks: contentBlocksToDisplay(content),
        isStreaming: false,
        timestamp: new Date().toISOString(),
      },
      {
        id: assistantMessageId,
        role: 'assistant' as const,
        blocks: [],
        isStreaming: true,
      },
    ];

    set((state) => ({
      chatStreams: {
        ...state.chatStreams,
        [streamId]: {
          streamId,
          sessionId,
          isStreaming: true,
          displayMessages: nextMessages,
          completedAt: null,
        },
      },
    }));

    return userMessageId;
  },

  setUserMessageDbId: (streamId, localMessageId, dbId) =>
    set((state) => {
      const stream = state.chatStreams[streamId];
      if (!stream) return state;
      return {
        chatStreams: {
          ...state.chatStreams,
          [streamId]: {
            ...stream,
            displayMessages: stream.displayMessages.map((message) =>
              message.id === localMessageId ? { ...message, dbId } : message
            ),
          },
        },
      };
    }),

  appendTextDelta: (streamId, delta) =>
    set((state) => {
      const baseStream = ensureStream(state, streamId);
      const [messages, assistantIndex] = getOrCreateAssistantMessage([
        ...baseStream.displayMessages.map(cloneDisplayMessage),
      ]);
      const assistant = { ...messages[assistantIndex] };
      const blocks = [...assistant.blocks];
      const lastBlock = blocks[blocks.length - 1];

      if (lastBlock && lastBlock.kind === 'text' && lastBlock.isStreaming) {
        blocks[blocks.length - 1] = { ...lastBlock, text: lastBlock.text + delta };
      } else {
        blocks.push({ kind: 'text', text: delta, isStreaming: true });
      }

      assistant.blocks = blocks;
      assistant.isStreaming = true;
      messages[assistantIndex] = assistant;

      return {
        chatStreams: {
          ...state.chatStreams,
          [streamId]: {
            ...baseStream,
            isStreaming: true,
            completedAt: null,
            displayMessages: messages,
          },
        },
      };
    }),

  addContentBlock: (streamId, payload) =>
    set((state) => {
      const baseStream = ensureStream(state, streamId);
      const [messages, assistantIndex] = getOrCreateAssistantMessage([
        ...baseStream.displayMessages.map(cloneDisplayMessage),
      ]);
      const assistant = { ...messages[assistantIndex] };
      const blocks = [...assistant.blocks];
      const lastBlock = blocks[blocks.length - 1];

      if (lastBlock && lastBlock.kind === 'text' && lastBlock.isStreaming) {
        blocks[blocks.length - 1] = { ...lastBlock, isStreaming: false };
      }

      let displayBlock: DisplayBlock | null = null;
      if (payload.block.type === 'thinking') {
        displayBlock = { kind: 'thinking', thinking: payload.block.thinking };
      } else if (payload.block.type === 'tool_use') {
        displayBlock = {
          kind: 'tool_call',
          id: payload.block.id,
          name: payload.block.name,
          input: payload.block.input,
        };
      }

      if (!displayBlock) return state;

      blocks.push(displayBlock);
      assistant.blocks = blocks;
      assistant.isStreaming = true;
      messages[assistantIndex] = assistant;

      return {
        chatStreams: {
          ...state.chatStreams,
          [streamId]: {
            ...baseStream,
            isStreaming: true,
            completedAt: null,
            displayMessages: messages,
          },
        },
      };
    }),

  addToolResult: (streamId, toolUseId, content, isError) =>
    set((state) => {
      const stream = state.chatStreams[streamId];
      if (!stream) return state;
      const messages = stream.displayMessages.map(cloneDisplayMessage);

      for (let i = messages.length - 1; i >= 0; i -= 1) {
        const message = messages[i];
        if (message.role !== 'assistant') continue;

        for (let j = message.blocks.length - 1; j >= 0; j -= 1) {
          const block = message.blocks[j];
          if (block.kind === 'tool_call' && block.id === toolUseId) {
            const nextBlocks = [...message.blocks];
            nextBlocks[j] = { ...block, result: { content, isError } };
            messages[i] = { ...message, blocks: nextBlocks };

            return {
              chatStreams: {
                ...state.chatStreams,
                [streamId]: {
                  ...stream,
                  displayMessages: messages,
                },
              },
            };
          }
        }
      }

      return state;
    }),

  addPermissionPrompt: (streamId, payload) =>
    set((state) => {
      const baseStream = ensureStream(state, streamId);
      const [messages, assistantIndex] = getOrCreateAssistantMessage([
        ...baseStream.displayMessages.map(cloneDisplayMessage),
      ]);
      const assistant = { ...messages[assistantIndex] };
      assistant.blocks = [
        ...assistant.blocks,
        {
          kind: 'permission_prompt',
          requestId: payload.requestId,
          toolName: payload.toolName,
          toolInput: payload.toolInput,
          riskLevel: payload.riskLevel,
          riskDescription: payload.riskDescription,
          suggestedPattern: payload.suggestedPattern,
        },
      ];
      assistant.isStreaming = true;
      messages[assistantIndex] = assistant;

      return {
        chatStreams: {
          ...state.chatStreams,
          [streamId]: {
            ...baseStream,
            isStreaming: true,
            completedAt: null,
            displayMessages: messages,
          },
        },
      };
    }),

  addUserQuestionPrompt: (streamId, payload) =>
    set((state) => {
      const baseStream = ensureStream(state, streamId);
      const [messages, assistantIndex] = getOrCreateAssistantMessage([
        ...baseStream.displayMessages.map(cloneDisplayMessage),
      ]);
      const assistant = { ...messages[assistantIndex] };
      assistant.blocks = [
        ...assistant.blocks,
        {
          kind: 'user_question_prompt',
          requestId: payload.requestId,
          question: payload.question,
          choices: payload.choices ?? undefined,
          allowCustom: payload.allowCustom,
          multiSelect: payload.multiSelect,
          context: payload.context ?? undefined,
        },
      ];
      assistant.isStreaming = true;
      messages[assistantIndex] = assistant;

      return {
        chatStreams: {
          ...state.chatStreams,
          [streamId]: {
            ...baseStream,
            isStreaming: true,
            completedAt: null,
            displayMessages: messages,
          },
        },
      };
    }),

  addReaction: (streamId, payload) =>
    set((state) => {
      const stream = state.chatStreams[streamId];
      if (!stream) return state;
      const messages = stream.displayMessages.map(cloneDisplayMessage);

      for (let i = 0; i < messages.length; i += 1) {
        if (messages[i].dbId === payload.messageId) {
          messages[i] = {
            ...messages[i],
            reactions: [
              ...(messages[i].reactions ?? []),
              { id: payload.reactionId, emoji: payload.emoji, isNew: true },
            ],
          };
          break;
        }
      }

      return {
        chatStreams: {
          ...state.chatStreams,
          [streamId]: {
            ...stream,
            displayMessages: messages,
          },
        },
      };
    }),

  handleIteration: (streamId, payload) =>
    set((state) => {
      const stream = state.chatStreams[streamId];
      if (!stream) return state;
      let messages = stream.displayMessages.map(cloneDisplayMessage);

      if (messages.length > 0 && (payload.action === 'llm_call' || payload.action === 'finished')) {
        const last = messages[messages.length - 1];
        if (last.role === 'assistant' && last.isStreaming) {
          messages[messages.length - 1] = finalizeStreamingMessage(last);
        }
      }

      if (payload.action === 'finished') {
        messages = maybeAppendCompletionMessage(messages, payload.finishSummary);
      }

      return {
        chatStreams: {
          ...state.chatStreams,
          [streamId]: {
            ...stream,
            isStreaming: payload.action !== 'finished',
            completedAt: payload.action === 'finished' ? Date.now() : null,
            displayMessages: messages,
          },
        },
      };
    }),

  completeChatStream: (streamId) =>
    set((state) => {
      const stream = state.chatStreams[streamId];
      if (!stream) return state;
      scheduleCleanup(streamId);
      const messages = stream.displayMessages.map(cloneDisplayMessage);
      const last = messages[messages.length - 1];
      if (last?.isStreaming) {
        messages[messages.length - 1] = finalizeStreamingMessage(last);
      }
      return {
        chatStreams: {
          ...state.chatStreams,
          [streamId]: {
            ...stream,
            isStreaming: false,
            completedAt: Date.now(),
            displayMessages: messages,
          },
        },
      };
    }),

  clearChatStream: (streamId) =>
    set((state) => {
      cancelCleanup(streamId);
      const { [streamId]: _removed, ...rest } = state.chatStreams;
      return { chatStreams: rest };
    }),
}));
