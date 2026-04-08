import { invoke } from '@tauri-apps/api/core';
import { PermissionRule } from '../types';

export const permissionsApi = {
  respond(requestId: string, response: 'allow' | 'always_allow' | 'deny') {
    return invoke('respond_to_permission', {
      requestId,
      response,
    });
  },

  saveRule(rule: PermissionRule) {
    return invoke('save_permission_rule', { agentId: null, rule });
  },

  deleteRule(ruleId: string) {
    return invoke('delete_permission_rule', { agentId: null, ruleId });
  },
};
