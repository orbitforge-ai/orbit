import { invoke } from './transport';

export type AuthStateDto =
  | { mode: 'unset' }
  | { mode: 'offline' }
  | { mode: 'cloud'; email: string };

export const authApi = {
  getAuthState: (): Promise<AuthStateDto> => invoke('get_auth_state'),

  login: (email: string, password: string): Promise<AuthStateDto> =>
    invoke('login', { email, password }),

  register: (email: string, password: string): Promise<AuthStateDto> =>
    invoke('register', { email, password }),

  logout: (): Promise<void> => invoke('logout'),

  setOfflineMode: (): Promise<void> => invoke('set_offline_mode'),

  forceSync: (): Promise<Record<string, number>> => invoke('force_cloud_sync'),
};
