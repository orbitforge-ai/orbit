import { create } from 'zustand';
import { ChatDraft, ChatSession } from '../types';

const STORAGE_KEY = 'orbit:chatDrafts';
const DRAFT_SESSION_PREFIX = '__draft__:';

type DraftMap = Record<string, ChatDraft>;

interface ChatDraftStore {
  drafts: DraftMap;
  ensureDraft: (agentId: string) => ChatDraft;
  updateDraftText: (agentId: string, text: string) => void;
  deleteDraft: (agentId: string) => void;
}

function loadDrafts(): DraftMap {
  if (typeof window === 'undefined') return {};

  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object') return {};
    return parsed as DraftMap;
  } catch {
    return {};
  }
}

function persistDrafts(drafts: DraftMap) {
  if (typeof window === 'undefined') return;

  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(drafts));
  } catch (err) {
    console.warn('Failed to persist chat drafts to localStorage:', err);
  }
}

function createDraft(agentId: string, text = ''): ChatDraft {
  const now = new Date().toISOString();
  return {
    id: `draft-${agentId}-${Date.now()}`,
    agentId,
    text,
    createdAt: now,
    updatedAt: now,
  };
}

export function getDraftSessionId(agentId: string) {
  return `${DRAFT_SESSION_PREFIX}${agentId}`;
}

export function isDraftSessionId(sessionId: string | null) {
  return Boolean(sessionId && sessionId.startsWith(DRAFT_SESSION_PREFIX));
}

export function draftToChatSession(draft: ChatDraft): ChatSession {
  return {
    id: getDraftSessionId(draft.agentId),
    agentId: draft.agentId,
    title: 'New Chat',
    archived: false,
    sessionType: 'user_chat',
    parentSessionId: null,
    sourceBusMessageId: null,
    chainDepth: 0,
    executionState: null,
    finishSummary: null,
    terminalError: null,
    sourceAgentId: null,
    sourceAgentName: null,
    sourceSessionId: null,
    sourceSessionTitle: null,
    createdAt: draft.createdAt,
    updatedAt: draft.updatedAt,
  };
}

export const useChatDraftStore = create<ChatDraftStore>((set, get) => ({
  drafts: loadDrafts(),

  ensureDraft: (agentId) => {
    const existing = get().drafts[agentId];
    if (existing) return existing;

    const draft = createDraft(agentId);
    const drafts = { ...get().drafts, [agentId]: draft };
    persistDrafts(drafts);
    set({ drafts });
    return draft;
  },

  updateDraftText: (agentId, text) => {
    const existing = get().drafts[agentId] ?? createDraft(agentId);
    const nextDraft: ChatDraft = {
      ...existing,
      text,
      updatedAt: new Date().toISOString(),
    };
    const drafts = { ...get().drafts, [agentId]: nextDraft };
    persistDrafts(drafts);
    set({ drafts });
  },

  deleteDraft: (agentId) => {
    if (!get().drafts[agentId]) return;
    const drafts = { ...get().drafts };
    delete drafts[agentId];
    persistDrafts(drafts);
    set({ drafts });
  },
}));
