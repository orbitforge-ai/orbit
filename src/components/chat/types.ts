export interface DisplayMessage {
  id: string;
  role: "user" | "assistant";
  blocks: DisplayBlock[];
  isStreaming: boolean;
}

export type DisplayBlock =
  | { kind: "text"; text: string; isStreaming: boolean }
  | { kind: "thinking"; thinking: string }
  | {
      kind: "tool_call";
      id: string;
      name: string;
      input: Record<string, unknown>;
      result?: { content: string; isError: boolean };
    }
  | { kind: "image"; mediaType: string; data: string };
