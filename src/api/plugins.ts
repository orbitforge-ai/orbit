import { invoke } from '@tauri-apps/api/core';

export interface PluginSummary {
  id: string;
  name: string;
  version: string;
  description: string | null;
  iconDataUrl: string | null;
  enabled: boolean;
  bundled: boolean;
  dev: boolean;
  running: boolean;
}

export interface PluginManifest {
  schemaVersion: number;
  hostApiVersion: string;
  id: string;
  name: string;
  version: string;
  description: string | null;
  author: string | null;
  homepage: string | null;
  license: string | null;
  icon: string | null;
  iconDataUrl: string | null;
  runtime: { type: string; command: string; args: string[]; workingDir: string | null; env: Record<string, string> };
  tools: Array<{ name: string; description: string | null; riskLevel: string; inputSchema: unknown | null }>;
  entityTypes: Array<{
    name: string;
    displayName: string | null;
    icon: string | null;
    schema: unknown;
    relations: Array<{ name: string; to: string; cardinality: string }>;
    listFields: string[];
    titleField: string | null;
    indexedFields: string[];
  }>;
  oauthProviders: Array<{
    id: string;
    name: string;
    authorizationUrl: string;
    tokenUrl: string;
    scopes: string[];
    clientType: string;
    redirectUri: string;
  }>;
  secrets: Array<{
    key: string;
    envVar: string;
    displayName: string;
    description: string | null;
    placeholder: string | null;
  }>;
  permissions: {
    network: string[];
    filesystem: string[];
    oauth: string[];
    coreEntities: string[];
  };
  hooks: { subscribe: string[] };
  workflow: {
    triggers: Array<{ kind: string; displayName: string; icon: string | null; configSchema: unknown | null; outputSchema: unknown | null; subscriptionTool: string | null }>;
    nodes: Array<{
      kind: string;
      displayName: string;
      icon: string | null;
      tool: string;
      fieldOptions: Array<{ field: string; sourceTool: string; format: string }>;
      inputSchema: unknown | null;
      outputSchema: unknown | null;
    }>;
  };
  ui: {
    sidebarItems: unknown[];
    entityDetailTabs: unknown[];
    agentChatActions: unknown[];
    slashCommands: unknown[];
    settingsPanels: unknown[];
  };
}

export interface StagedInstall {
  stagingId: string;
  manifest: PluginManifest;
}

export interface PluginOAuthProviderStatus {
  id: string;
  name: string;
  clientType: string;
  connected: boolean;
  hasClientId: boolean;
}

export interface PluginOAuthStatus {
  pluginId: string;
  anyNeedsConnect: boolean;
  providers: PluginOAuthProviderStatus[];
}

export interface PluginSecretEntryStatus {
  key: string;
  displayName: string;
  description: string | null;
  placeholder: string | null;
  hasValue: boolean;
}

export interface PluginSecretStatus {
  pluginId: string;
  anyNeedsValue: boolean;
  secrets: PluginSecretEntryStatus[];
}

export interface PluginEntity {
  id: string;
  pluginId: string;
  entityType: string;
  projectId: string | null;
  data: unknown;
  createdByAgentId: string | null;
  createdAt: string;
  updatedAt: string;
}

export const pluginsApi = {
  list: (): Promise<PluginSummary[]> => invoke('list_plugins'),

  getManifest: (pluginId: string): Promise<PluginManifest | null> =>
    invoke('get_plugin_manifest', { pluginId }),

  callTool: (pluginId: string, toolName: string, args: Record<string, unknown> = {}): Promise<unknown> =>
    invoke('plugin_call_tool', { pluginId, toolName, args }),

  stageInstall: (path: string): Promise<StagedInstall> =>
    invoke('stage_plugin_install', { path }),

  confirmInstall: (stagingId: string): Promise<PluginManifest> =>
    invoke('confirm_plugin_install', { stagingId }),

  cancelInstall: (stagingId: string): Promise<void> =>
    invoke('cancel_plugin_install', { stagingId }),

  installFromDirectory: (path: string): Promise<PluginManifest> =>
    invoke('install_plugin_from_directory', { path }),

  setEnabled: (pluginId: string, enabled: boolean): Promise<void> =>
    invoke('set_plugin_enabled', { pluginId, enabled }),

  reload: (pluginId: string): Promise<void> =>
    invoke('reload_plugin', { pluginId }),

  reloadAll: (): Promise<void> => invoke('reload_all_plugins'),

  uninstall: (pluginId: string): Promise<void> =>
    invoke('uninstall_plugin', { pluginId }),

  setOAuthConfig: (
    pluginId: string,
    providerId: string,
    clientId: string,
    clientSecret?: string
  ): Promise<void> =>
    invoke('set_plugin_oauth_config', { pluginId, providerId, clientId, clientSecret: clientSecret ?? null }),

  startOAuth: (pluginId: string, providerId: string): Promise<void> =>
    invoke('start_plugin_oauth', { pluginId, providerId }),

  disconnectOAuth: (pluginId: string, providerId: string): Promise<void> =>
    invoke('disconnect_plugin_oauth', { pluginId, providerId }),

  getRuntimeLog: (pluginId: string, tailLines?: number): Promise<string> =>
    invoke('get_plugin_runtime_log', { pluginId, tailLines: tailLines ?? 200 }),

  listEntities: (
    pluginId: string,
    entityType: string,
    opts: { projectId?: string; limit?: number; offset?: number } = {}
  ): Promise<PluginEntity[]> =>
    invoke('list_plugin_entities', {
      pluginId,
      entityType,
      projectId: opts.projectId ?? null,
      limit: opts.limit ?? null,
      offset: opts.offset ?? null,
    }),

  getEntity: (id: string): Promise<PluginEntity | null> =>
    invoke('get_plugin_entity', { id }),

  listOAuthStatus: (): Promise<PluginOAuthStatus[]> =>
    invoke('list_plugin_oauth_status'),

  setSecret: (pluginId: string, key: string, value: string): Promise<void> =>
    invoke('set_plugin_secret', { pluginId, key, value }),

  deleteSecret: (pluginId: string, key: string): Promise<void> =>
    invoke('delete_plugin_secret', { pluginId, key }),

  listSecretStatus: (): Promise<PluginSecretStatus[]> =>
    invoke('list_plugin_secret_status'),
};
