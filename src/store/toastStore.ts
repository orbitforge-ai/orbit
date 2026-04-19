import { create } from 'zustand';

export type ToastKind = 'success' | 'error' | 'info';

export interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
  detail?: string;
}

interface ToastStore {
  toasts: Toast[];
  push: (kind: ToastKind, message: string, detail?: string) => void;
  dismiss: (id: number) => void;
}

const AUTO_DISMISS_MS = 3000;
let nextId = 1;

export const useToastStore = create<ToastStore>((set, get) => ({
  toasts: [],
  push: (kind, message, detail) => {
    const id = nextId++;
    set((s) => ({ toasts: [...s.toasts, { id, kind, message, detail }] }));
    setTimeout(() => get().dismiss(id), AUTO_DISMISS_MS);
  },
  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));

function extractDetail(err: unknown): string | undefined {
  if (err == null) return undefined;
  if (typeof err === 'string') return err;
  if (err instanceof Error) return err.message;
  try {
    return JSON.stringify(err);
  } catch {
    return String(err);
  }
}

export const toast = {
  success: (message: string) => useToastStore.getState().push('success', message),
  error: (message: string, err?: unknown) =>
    useToastStore.getState().push('error', message, extractDetail(err)),
  info: (message: string) => useToastStore.getState().push('info', message),
};
