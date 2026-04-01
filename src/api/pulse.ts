import { invoke } from '@tauri-apps/api/core';
import { RecurringConfig } from '../types';

export interface PulseConfig {
  enabled: boolean;
  content: string;
  schedule: RecurringConfig | null;
  taskId: string | null;
  scheduleId: string | null;
  sessionId: string | null;
  nextRunAt: string | null;
  lastRunAt: string | null;
}

export const pulseApi = {
  getConfig: (agentId: string): Promise<PulseConfig> => invoke('get_pulse_config', { agentId }),

  update: (
    agentId: string,
    content: string,
    scheduleConfig: RecurringConfig,
    enabled: boolean
  ): Promise<PulseConfig> => invoke('update_pulse', { agentId, content, scheduleConfig, enabled }),
};
