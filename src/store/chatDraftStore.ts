import { create } from 'zustand';
import { ChatDraft, ChatSession } from '../types';

const STORAGE_KEY = 'orbit:chatDrafts';
const DRAFT_SESSION_PREFIX = '__draft__:';

type DraftMap = Record<string, ChatDraft>;

export interface DraftScope {
  agentId: string;
  projectId?: string | null;
}

interface ChatDraftStore {
  drafts: DraftMap;
  ensureDraft: (scope: DraftScope) => ChatDraft;
  updateDraftText: (scope: DraftScope, text: string) => void;
  deleteDraft: (scope: DraftScope) => void;
}

function scopeKey({ agentId, projectId }: DraftScope): string {
  return `${projectId ?? 'global'}:${agentId}`;
}

export function getDraftScopeKey(scope: DraftScope): string {
  return scopeKey(scope);
}

export function getDraftSessionId(scope: DraftScope) {
  return `${DRAFT_SESSION_PREFIX}${scopeKey(scope)}`;
}

export function isDraftSessionId(sessionId: string | null) {
  return Boolean(sessionId && sessionId.startsWith(DRAFT_SESSION_PREFIX));
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

function createDraft(scope: DraftScope, text = ''): ChatDraft {
  const now = new Date().toISOString();
  return {
    id: `draft-${scopeKey(scope)}-${Date.now()}`,
    agentId: scope.agentId,
    projectId: scope.projectId ?? null,
    text,
    createdAt: now,
    updatedAt: now,
  };
}

export function draftToChatSession(draft: ChatDraft): ChatSession {
  return {
    id: getDraftSessionId({ agentId: draft.agentId, projectId: draft.projectId }),
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
    projectId: draft.projectId,
  };
}

export const useChatDraftStore = create<ChatDraftStore>((set, get) => ({
  drafts: loadDrafts(),

  ensureDraft: (scope) => {
    const key = scopeKey(scope);
    const existing = get().drafts[key];
    if (existing) return existing;

    const draft = createDraft(scope);
    const drafts = { ...get().drafts, [key]: draft };
    persistDrafts(drafts);
    set({ drafts });
    return draft;
  },

  updateDraftText: (scope, text) => {
    const key = scopeKey(scope);
    const existing = get().drafts[key] ?? createDraft(scope);
    const nextDraft: ChatDraft = {
      ...existing,
      text,
      updatedAt: new Date().toISOString(),
    };
    const drafts = { ...get().drafts, [key]: nextDraft };
    persistDrafts(drafts);
    set({ drafts });
  },

  deleteDraft: (scope) => {
    const key = scopeKey(scope);
    if (!get().drafts[key]) return;
    const drafts = { ...get().drafts };
    delete drafts[key];
    persistDrafts(drafts);
    set({ drafts });
  },
}));
