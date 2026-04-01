import { invoke } from '@tauri-apps/api/core';
import { SkillInfo } from '../types';

export const skillsApi = {
  list: (agentId: string): Promise<SkillInfo[]> => invoke('list_skills', { agentId }),

  getContent: (agentId: string, skillName: string): Promise<string> =>
    invoke('get_skill_content', { agentId, skillName }),

  create: (agentId: string, name: string, description: string, body: string): Promise<void> =>
    invoke('create_skill', { agentId, name, description, body }),

  delete: (agentId: string, skillName: string): Promise<void> =>
    invoke('delete_skill', { agentId, skillName }),
};
