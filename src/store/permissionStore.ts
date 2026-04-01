import { create } from 'zustand';
import { PermissionRequestPayload } from '../types';

interface PermissionStore {
  /** All pending permission requests, keyed by requestId. */
  pending: Record<string, PermissionRequestPayload>;
  /** Count of pending requests (for notification badge). */
  pendingCount: number;

  addRequest: (req: PermissionRequestPayload) => void;
  removeRequest: (requestId: string) => void;
  removeForRun: (runId: string) => void;
  /** Mark a request as resolved (keeps it briefly for UI display). */
  resolveRequest: (requestId: string, decision: 'allow' | 'always_allow' | 'deny') => void;
}

export const usePermissionStore = create<PermissionStore>((set) => ({
  pending: {},
  pendingCount: 0,

  addRequest: (req) =>
    set((state) => {
      const pending = { ...state.pending, [req.requestId]: req };
      return { pending, pendingCount: Object.keys(pending).length };
    }),

  removeRequest: (requestId) =>
    set((state) => {
      const { [requestId]: _, ...rest } = state.pending;
      return { pending: rest, pendingCount: Object.keys(rest).length };
    }),

  removeForRun: (runId) =>
    set((state) => {
      const pending: Record<string, PermissionRequestPayload> = {};
      for (const [id, req] of Object.entries(state.pending)) {
        if (req.runId !== runId) {
          pending[id] = req;
        }
      }
      return { pending, pendingCount: Object.keys(pending).length };
    }),

  resolveRequest: (requestId, _decision) =>
    set((state) => {
      const { [requestId]: _, ...rest } = state.pending;
      return { pending: rest, pendingCount: Object.keys(rest).length };
    }),
}));
