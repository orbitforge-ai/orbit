import { ChatMessage, ContentBlock } from "../../types";
import { DisplayMessage, DisplayBlock } from "./types";

let idCounter = 0;
function nextId(): string {
  return `msg-${++idCounter}`;
}

/**
 * Convert a ChatMessage[] (from DB) into DisplayMessage[] for rendering.
 * Merges tool_result blocks into the preceding assistant message's tool_call blocks.
 */
export function chatMessagesToDisplay(messages: ChatMessage[]): DisplayMessage[] {
  const result: DisplayMessage[] = [];

  for (let i = 0; i < messages.length; i++) {
    const msg = messages[i];

    // tool_result messages (role=user with only tool_result content) get merged
    // into the previous assistant message's tool_call blocks
    const isToolResultMessage =
      msg.role === "user" &&
      msg.content.length > 0 &&
      msg.content.every((b) => b.type === "tool_result");

    if (isToolResultMessage && result.length > 0) {
      const prev = result[result.length - 1];
      if (prev.role === "assistant") {
        for (const block of msg.content) {
          if (block.type === "tool_result") {
            const toolCall = prev.blocks.find(
              (b) => b.kind === "tool_call" && b.id === block.tool_use_id
            );
            if (toolCall && toolCall.kind === "tool_call") {
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
    for (const block of msg.content) {
      blocks.push(contentBlockToDisplay(block));
    }

    result.push({
      id: nextId(),
      role: msg.role,
      blocks,
      isStreaming: false,
    });
  }

  return result;
}

function contentBlockToDisplay(block: ContentBlock): DisplayBlock {
  switch (block.type) {
    case "text":
      return { kind: "text", text: block.text, isStreaming: false };
    case "thinking":
      return { kind: "thinking", thinking: block.thinking };
    case "tool_use":
      return {
        kind: "tool_call",
        id: block.id,
        name: block.name,
        input: block.input,
      };
    case "tool_result":
      // Standalone tool_result (shouldn't happen after merge, but handle gracefully)
      return {
        kind: "text",
        text: `[Tool result: ${block.content}]`,
        isStreaming: false,
      };
    case "image":
      return { kind: "image", mediaType: block.media_type, data: block.data };
  }
}
