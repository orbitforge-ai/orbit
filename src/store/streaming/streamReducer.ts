import { AgentContentBlockPayload, ContentBlock } from '../../types';
import { DisplayBlock, DisplayMessage } from '../../components/chat/types';

export interface ToolResultData {
  content: string;
  isError: boolean;
}

export interface StreamPreviewState {
  previewMessage: DisplayMessage | null;
  pendingToolResults: Record<string, ToolResultData>;
}

interface NormalizedThinkingEvent {
  kind: 'thinking';
  thinking: string;
  isStreaming: boolean;
}

interface NormalizedToolUseEvent {
  kind: 'tool_use';
  id: string;
  name: string;
  input: Record<string, unknown>;
  inputText: string | null;
  isStreaming: boolean;
}

interface NormalizedToolResultEvent {
  kind: 'tool_result';
  toolUseId: string;
  content: string;
  isError: boolean;
}

type NormalizedContentEvent =
  | NormalizedThinkingEvent
  | NormalizedToolUseEvent
  | NormalizedToolResultEvent;

function cloneBlock(block: DisplayBlock): DisplayBlock {
  switch (block.kind) {
    case 'text':
      return { ...block };
    case 'thinking':
      return { ...block };
    case 'tool_call':
      return {
        ...block,
        input: { ...block.input },
        result: block.result ? { ...block.result } : undefined,
      };
    case 'image':
      return { ...block };
    case 'permission_prompt':
      return { ...block, toolInput: { ...block.toolInput } };
    case 'user_question_prompt':
      return { ...block, choices: block.choices ? [...block.choices] : undefined };
  }
}

export function cloneDisplayMessage(message: DisplayMessage): DisplayMessage {
  return {
    ...message,
    blocks: message.blocks.map(cloneBlock),
    reactions: message.reactions?.map((reaction) => ({ ...reaction })),
  };
}

export function createEmptyPreviewState(): StreamPreviewState {
  return {
    previewMessage: null,
    pendingToolResults: {},
  };
}

export function createAssistantPreview(
  nextMessageId: () => string
): DisplayMessage {
  return {
    id: nextMessageId(),
    role: 'assistant',
    blocks: [],
    isStreaming: true,
    timestamp: new Date().toISOString(),
  };
}

export function contentBlocksToDisplay(content: ContentBlock[]): DisplayBlock[] {
  return content.map((block): DisplayBlock => {
    if (block.type === 'text') return { kind: 'text', text: block.text, isStreaming: false };
    if (block.type === 'thinking') {
      return { kind: 'thinking', thinking: block.thinking, isStreaming: false };
    }
    if (block.type === 'tool_use') {
      return {
        kind: 'tool_call',
        id: block.id,
        name: block.name,
        input: block.input,
        inputText: JSON.stringify(block.input, null, 2),
        isStreaming: false,
      };
    }
    if (block.type === 'image') {
      return { kind: 'image', mediaType: block.media_type, data: block.data };
    }
    return { kind: 'text', text: '[attachment]', isStreaming: false };
  });
}

function ensurePreviewMessage(
  previewMessage: DisplayMessage | null,
  nextMessageId: () => string
): DisplayMessage {
  return previewMessage ? cloneDisplayMessage(previewMessage) : createAssistantPreview(nextMessageId);
}

function finalizeStreamingBlocks(blocks: DisplayBlock[]): DisplayBlock[] {
  return blocks.map((block) => {
    if (block.kind === 'text' && block.isStreaming) {
      return { ...block, isStreaming: false };
    }
    if (block.kind === 'thinking' && block.isStreaming) {
      return { ...block, isStreaming: false };
    }
    if (block.kind === 'tool_call' && block.isStreaming) {
      return { ...block, isStreaming: false };
    }
    return cloneBlock(block);
  });
}

export function finalizePreviewMessage(
  previewMessage: DisplayMessage,
  keepMessageStreaming = false
): DisplayMessage {
  return {
    ...cloneDisplayMessage(previewMessage),
    blocks: finalizeStreamingBlocks(previewMessage.blocks),
    isStreaming: keepMessageStreaming,
  };
}

function hasPrimaryContent(message: DisplayMessage | null | undefined): boolean {
  if (!message) return false;
  return message.blocks.some((block) => block.kind !== 'thinking' && block.kind !== 'tool_call');
}

export function appendCompletionSummary(
  messages: DisplayMessage[],
  finishSummary: string | null | undefined,
  nextMessageId: () => string
): DisplayMessage[] {
  const summary = finishSummary?.trim();
  if (!summary) return messages;

  const last = messages[messages.length - 1];
  if (last?.role === 'assistant' && !hasPrimaryContent(last)) {
    return [
      ...messages,
      {
        id: nextMessageId(),
        role: 'assistant',
        blocks: [{ kind: 'text', text: summary, isStreaming: false }],
        isStreaming: false,
        timestamp: new Date().toISOString(),
      },
    ];
  }

  return messages;
}

export function appendTextDelta(
  state: StreamPreviewState,
  delta: string,
  nextMessageId: () => string
): StreamPreviewState {
  const previewMessage = ensurePreviewMessage(state.previewMessage, nextMessageId);
  const blocks = [...previewMessage.blocks];
  const lastBlock = blocks[blocks.length - 1];

  if (lastBlock && lastBlock.kind === 'text' && lastBlock.isStreaming) {
    blocks[blocks.length - 1] = { ...lastBlock, text: lastBlock.text + delta };
  } else {
    blocks.push({ kind: 'text', text: delta, isStreaming: true });
  }

  return {
    ...state,
    previewMessage: {
      ...previewMessage,
      blocks,
      isStreaming: true,
    },
  };
}

function normalizeContentEvent(payload: AgentContentBlockPayload): NormalizedContentEvent | null {
  const blockType = payload.blockType;
  const block = payload.block;

  if (
    ('name' in block && block.name === 'react_to_message') ||
    (blockType === 'tool_input_delta' && 'name' in block && block.name === 'react_to_message')
  ) {
    return null;
  }

  if (blockType === 'tool_result' && block.type === 'tool_result') {
    return {
      kind: 'tool_result',
      toolUseId: block.tool_use_id,
      content: block.content,
      isError: block.is_error,
    };
  }

  if (blockType === 'thinking_delta' && block.type === 'thinking_delta') {
    return { kind: 'thinking', thinking: block.thinking, isStreaming: true };
  }

  if (blockType === 'thinking' && block.type === 'thinking') {
    return { kind: 'thinking', thinking: block.thinking, isStreaming: false };
  }

  if (blockType === 'tool_input_delta' && block.type === 'tool_input_delta') {
    return {
      kind: 'tool_use',
      id: block.id,
      name: block.name,
      input: {},
      inputText: block.partial_json,
      isStreaming: true,
    };
  }

  if (blockType === 'tool_use' && block.type === 'tool_use') {
    return {
      kind: 'tool_use',
      id: block.id,
      name: block.name,
      input: block.input,
      inputText: JSON.stringify(block.input, null, 2),
      isStreaming: false,
    };
  }

  return null;
}

function attachToolResultToBlocks(
  blocks: DisplayBlock[],
  toolUseId: string,
  result: ToolResultData
): { blocks: DisplayBlock[]; attached: boolean } {
  for (let i = blocks.length - 1; i >= 0; i -= 1) {
    const block = blocks[i];
    if (block.kind === 'tool_call' && block.id === toolUseId) {
      const nextBlocks = [...blocks];
      nextBlocks[i] = { ...block, result: { ...result } };
      return { blocks: nextBlocks, attached: true };
    }
  }

  return { blocks, attached: false };
}

export function applyToolResult(
  messages: DisplayMessage[],
  state: StreamPreviewState,
  toolUseId: string,
  content: string,
  isError: boolean
): {
  messages: DisplayMessage[];
  state: StreamPreviewState;
} {
  const result = { content, isError };

  if (state.previewMessage) {
    const previewMessage = cloneDisplayMessage(state.previewMessage);
    const attachedPreview = attachToolResultToBlocks(previewMessage.blocks, toolUseId, result);
    if (attachedPreview.attached) {
      return {
        messages,
        state: {
          ...state,
          previewMessage: { ...previewMessage, blocks: attachedPreview.blocks },
        },
      };
    }
  }

  const nextMessages = messages.map(cloneDisplayMessage);
  for (let i = nextMessages.length - 1; i >= 0; i -= 1) {
    const message = nextMessages[i];
    if (message.role !== 'assistant') continue;
    const attached = attachToolResultToBlocks(message.blocks, toolUseId, result);
    if (attached.attached) {
      nextMessages[i] = { ...message, blocks: attached.blocks };
      return {
        messages: nextMessages,
        state,
      };
    }
  }

  return {
    messages,
    state: {
      ...state,
      pendingToolResults: {
        ...state.pendingToolResults,
        [toolUseId]: result,
      },
    },
  };
}

export function applyContentBlock(
  state: StreamPreviewState,
  payload: AgentContentBlockPayload,
  nextMessageId: () => string
): StreamPreviewState {
  const normalized = normalizeContentEvent(payload);
  if (!normalized) return state;

  if (normalized.kind === 'tool_result') {
    const attached = applyToolResult([], state, normalized.toolUseId, normalized.content, normalized.isError);
    return attached.state;
  }

  const previewMessage = ensurePreviewMessage(state.previewMessage, nextMessageId);
  const blocks = [...previewMessage.blocks];
  const lastBlock = blocks[blocks.length - 1];

  if (lastBlock && lastBlock.kind === 'text' && lastBlock.isStreaming) {
    blocks[blocks.length - 1] = { ...lastBlock, isStreaming: false };
  }

  if (normalized.kind === 'thinking') {
    const thinkingIndex = [...blocks]
      .reverse()
      .findIndex((block) => block.kind === 'thinking' && block.isStreaming);

    if (thinkingIndex >= 0) {
      const actualIndex = blocks.length - 1 - thinkingIndex;
      const existing = blocks[actualIndex] as Extract<DisplayBlock, { kind: 'thinking' }>;
      blocks[actualIndex] = {
        ...existing,
        thinking: normalized.isStreaming
          ? existing.thinking + normalized.thinking
          : normalized.thinking,
        isStreaming: normalized.isStreaming,
      };
    } else {
      blocks.push({
        kind: 'thinking',
        thinking: normalized.thinking,
        isStreaming: normalized.isStreaming,
      });
    }
  }

  if (normalized.kind === 'tool_use') {
    const toolIndex = blocks.findIndex(
      (block) => block.kind === 'tool_call' && block.id === normalized.id
    );
    const pendingResult = state.pendingToolResults[normalized.id];

    if (toolIndex >= 0) {
      const existing = blocks[toolIndex] as Extract<DisplayBlock, { kind: 'tool_call' }>;
      blocks[toolIndex] = {
        ...existing,
        name: normalized.name,
        input: normalized.isStreaming ? existing.input : normalized.input,
        inputText: normalized.isStreaming
          ? `${existing.inputText ?? ''}${normalized.inputText ?? ''}`
          : normalized.inputText ?? existing.inputText,
        isStreaming: normalized.isStreaming,
        result: existing.result ?? pendingResult,
      };
    } else {
      blocks.push({
        kind: 'tool_call',
        id: normalized.id,
        name: normalized.name,
        input: normalized.input,
        inputText: normalized.inputText ?? undefined,
        isStreaming: normalized.isStreaming,
        result: pendingResult,
      });
    }

    const { [normalized.id]: _ignored, ...remainingResults } = state.pendingToolResults;
    return {
      previewMessage: {
        ...previewMessage,
        blocks,
        isStreaming: true,
      },
      pendingToolResults: remainingResults,
    };
  }

  return {
    ...state,
    previewMessage: {
      ...previewMessage,
      blocks,
      isStreaming: true,
    },
  };
}

export function appendPreviewBlock(
  state: StreamPreviewState,
  block: DisplayBlock,
  nextMessageId: () => string
): StreamPreviewState {
  const previewMessage = ensurePreviewMessage(state.previewMessage, nextMessageId);
  const blocks = [...previewMessage.blocks];
  const lastBlock = blocks[blocks.length - 1];

  if (lastBlock && lastBlock.kind === 'text' && lastBlock.isStreaming) {
    blocks[blocks.length - 1] = { ...lastBlock, isStreaming: false };
  }

  blocks.push(cloneBlock(block));

  return {
    ...state,
    previewMessage: {
      ...previewMessage,
      blocks,
      isStreaming: true,
    },
  };
}

export function commitPreviewMessage(
  messages: DisplayMessage[],
  state: StreamPreviewState,
  finishSummary: string | null | undefined,
  nextMessageId: () => string
): {
  messages: DisplayMessage[];
  state: StreamPreviewState;
} {
  const nextMessages = messages.map(cloneDisplayMessage);
  const previewMessage = state.previewMessage ? finalizePreviewMessage(state.previewMessage) : null;

  if (previewMessage && previewMessage.blocks.length > 0) {
    nextMessages.push(previewMessage);
  }

  return {
    messages: appendCompletionSummary(nextMessages, finishSummary, nextMessageId),
    state: {
      previewMessage: null,
      pendingToolResults: state.pendingToolResults,
    },
  };
}
