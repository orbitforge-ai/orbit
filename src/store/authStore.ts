import { create } from 'zustand';
import { authApi, AuthStateDto } from '../api/auth';

interface AuthStore {
  state: AuthStateDto | null; // null = not yet loaded
  isLoading: boolean;
  error: string | null;

  load: () => Promise<void>;
  login: (email: string, password: string) => Promise<void>;
  logout: () => Promise<void>;
  continueOffline: () => Promise<void>;
}

export const useAuthStore = create<AuthStore>((set) => ({
  state: null,
  isLoading: false,
  error: null,

  load: async () => {
    set({ isLoading: true, error: null });
    try {
      const state = await authApi.getAuthState();
      set({ state, isLoading: false });
    } catch (e) {
      set({ isLoading: false, error: String(e) });
    }
  },

  login: async (email, password) => {
    set({ isLoading: true, error: null });
    try {
      const state = await authApi.login(email, password);
      set({ state, isLoading: false });
    } catch (e) {
      set({ isLoading: false, error: String(e) });
      throw e;
    }
  },

  logout: async () => {
    set({ isLoading: true, error: null });
    try {
      await authApi.logout();
      set({ state: { mode: 'unset' }, isLoading: false });
    } catch (e) {
      set({ isLoading: false, error: String(e) });
    }
  },

  continueOffline: async () => {
    set({ isLoading: true, error: null });
    try {
      await authApi.setOfflineMode();
      set({ state: { mode: 'offline' }, isLoading: false });
    } catch (e) {
      set({ isLoading: false, error: String(e) });
    }
  },
}));
