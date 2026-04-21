import { ChatMessage, ContentBlock } from '../../types';
import { DisplayMessage, DisplayBlock } from './types';

let idCounter = 0;
function nextId(): string {
  return `msg-${++idCounter}`;
}

const AGENT_MESSAGE_WRAPPER =
  /^<agent_message from="([^"]+)" untrusted="true">([\s\S]*)<\/agent_message>$/;

/**
 * Detect and unwrap a cross-agent message wrapper.
 * Returns the inner text and the sender agent id, or null if not wrapped.
 */
function unwrapAgentMessage(text: string): { from: string; inner: string } | null {
  const match = text.trim().match(AGENT_MESSAGE_WRAPPER);
  if (!match) return null;
  return { from: match[1], inner: match[2] };
}

/**
 * Convert a ChatMessage[] (from DB) into DisplayMessage[] for rendering.
 * Merges tool_result blocks into the preceding assistant message's tool_call blocks.
 */
export function chatMessagesToDisplay(messages: ChatMessage[]): DisplayMessage[] {
  const result: DisplayMessage[] = [];
  const reactToolIds = new Set<string>();

  for (let i = 0; i < messages.length; i++) {
    const msg = messages[i];

    // tool_result messages (role=user with only tool_result content) get merged
    // into the previous assistant message's tool_call blocks
    const isToolResultMessage =
      msg.role === 'user' &&
      msg.content.length > 0 &&
      msg.content.every((b) => b.type === 'tool_result');

    if (isToolResultMessage && result.length > 0) {
      const prev = result[result.length - 1];
      if (prev.role === 'assistant') {
        for (const block of msg.content) {
          if (block.type === 'tool_result') {
            // Skip results for react_to_message (their tool_use was already filtered)
            if (reactToolIds.has(block.tool_use_id)) continue;
            const toolCall = prev.blocks.find(
              (b) => b.kind === 'tool_call' && b.id === block.tool_use_id
            );
            if (toolCall && toolCall.kind === 'tool_call') {
              toolCall.result = {
                content: block.content,
                isError: block.is_error,
              };
            }
          }
        }
        continue;
      }
    }

    const blocks: DisplayBlock[] = [];
    let senderLabel: string | undefined;
    for (const block of msg.content) {
      // Filter out react_to_message tool calls (hidden — no bubble)
      if (block.type === 'tool_use' && block.name === 'react_to_message') {
        reactToolIds.add(block.id);
        continue;
      }
      // Unwrap cross-agent message envelopes on user-role text blocks so the
      // bubble shows the inner content with a sender label rather than raw XML.
      if (msg.role === 'user' && block.type === 'text') {
        const unwrapped = unwrapAgentMessage(block.text);
        if (unwrapped) {
          if (!senderLabel) senderLabel = unwrapped.from;
          blocks.push({ kind: 'text', text: unwrapped.inner, isStreaming: false });
          continue;
        }
      }
      blocks.push(contentBlockToDisplay(block));
    }

    // Detect summary messages by content marker
    const isSummary =
      msg.content.length > 0 &&
      msg.content[0].type === 'text' &&
      msg.content[0].text.startsWith('[Conversation Summary]');

    result.push({
      id: nextId(),
      dbId: msg.id,
      role: msg.role,
      blocks,
      isStreaming: false,
      timestamp: msg.created_at,
      isCompacted: msg.isCompacted,
      isSummary,
      senderLabel,
    });
  }

  return result;
}

function contentBlockToDisplay(block: ContentBlock): DisplayBlock {
  switch (block.type) {
    case 'text':
      return { kind: 'text', text: block.text, isStreaming: false };
    case 'thinking':
      return { kind: 'thinking', thinking: block.thinking, isStreaming: false };
    case 'tool_use':
      return {
        kind: 'tool_call',
        id: block.id,
        name: block.name,
        input: block.input,
        inputText: JSON.stringify(block.input, null, 2),
        isStreaming: false,
      };
    case 'tool_result':
      // Standalone tool_result (shouldn't happen after merge, but handle gracefully)
      return {
        kind: 'text',
        text: `[Tool result: ${block.content}]`,
        isStreaming: false,
      };
    case 'image':
      return { kind: 'image', mediaType: block.media_type, data: block.data };
  }
}
