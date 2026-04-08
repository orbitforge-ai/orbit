import { invoke } from '@tauri-apps/api/core';
import { GlobalSettings } from '../types';

export const globalSettingsApi = {
  get(): Promise<GlobalSettings> {
    return invoke('get_global_settings');
  },

  update(settings: GlobalSettings): Promise<GlobalSettings> {
    return invoke('update_global_settings', { settings });
  },
};
