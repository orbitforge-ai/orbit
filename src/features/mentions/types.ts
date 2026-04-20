export type MentionKind = 'agent' | 'file' | 'item';

export interface MentionToken {
  kind: MentionKind;
  label: string;
  // agent: agentId
  // file:  `${agentId}:${relPath}`
  // item:  workItemId
  payload: string;
}

export interface PickerContext {
  agentId: string;
  projectId: string | null;
}

export interface MentionGroup {
  kind: MentionKind;
  title: string;
  items: MentionItem[];
  truncated?: boolean;
}

export interface MentionItem {
  id: string;
  label: string;
  secondary?: string;
  token: MentionToken;
  __selected?: boolean;
}
