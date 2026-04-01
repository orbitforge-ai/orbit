export interface DisplayMessage {
  id: string;
  role: "user" | "assistant";
  blocks: DisplayBlock[];
  isStreaming: boolean;
  timestamp?: string; // ISO 8601
  isCompacted?: boolean;
  isSummary?: boolean;
  senderLabel?: string;   // agent name for bus messages (shown instead of "User" icon)
  linkedRunId?: string;    // run ID to link to from assistant bus responses
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
  | { kind: "image"; mediaType: string; data: string }
  | {
      kind: "permission_prompt";
      requestId: string;
      toolName: string;
      toolInput: Record<string, unknown>;
      riskLevel: "moderate" | "dangerous";
      riskDescription: string;
      suggestedPattern: string;
      resolved?: "allow" | "always_allow" | "deny";
    };
