import { useEffect, useState, useImperativeHandle, forwardRef } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Key, Trash2, Check, ChevronDown } from 'lucide-react';
import * as Select from '@radix-ui/react-select';
import * as Slider from '@radix-ui/react-slider';
import * as Switch from '@radix-ui/react-switch';

import { workspaceApi } from '../../api/workspace';
import { llmApi } from '../../api/llm';
import { permissionsApi } from '../../api/permissions';
import { AgentWorkspaceConfig } from '../../types';
import { confirm } from '@tauri-apps/plugin-dialog';
import { CollapsibleSection } from '../../components/CollapsibleSection';
import { AgentIdentitySection } from './AgentIdentitySection';
import { RoleSelector } from './RoleSelector';
import {
  getRoleDefaultTools,
  getRoleSystemInstructions,
  DEFAULT_ROLE_ID,
} from '../../lib/agentRoles';

const PERMISSION_MODES = [
  { value: 'normal', label: 'Normal', description: 'Prompt for writes/exec, auto-allow reads' },
  { value: 'strict', label: 'Strict', description: 'Prompt for all non-read operations' },
  {
    value: 'permissive',
    label: 'Permissive',
    description: 'Auto-allow everything (advanced users)',
  },
];

const TOOL_CATEGORIES = [
  {
    label: 'File System',
    tools: [
      { id: 'read_file', label: 'Read Files' },
      { id: 'write_file', label: 'Write Files' },
      { id: 'list_files', label: 'List Files' },
    ],
  },
  {
    label: 'Execution',
    tools: [{ id: 'shell_command', label: 'Shell Commands' }],
  },
  {
    label: 'Communication',
    tools: [
      { id: 'send_message', label: 'Send Message' },
      { id: 'web_search', label: 'Web Search' },
    ],
  },
  {
    label: 'Agent Control',
    tools: [
      { id: 'spawn_sub_agents', label: 'Sub-Agents' },
      { id: 'activate_skill', label: 'Activate Skill' },
    ],
  },
  {
    label: 'Memory',
    tools: [
      { id: 'remember', label: 'Remember' },
      { id: 'search_memory', label: 'Search Memory' },
      { id: 'forget', label: 'Forget' },
      { id: 'list_memories', label: 'List Memories' },
    ],
  },
];

const ALL_TOOL_IDS = TOOL_CATEGORIES.flatMap((c) => c.tools.map((t) => t.id));

const SEARCH_PROVIDERS = [
  { value: 'brave', label: 'Brave Search' },
  { value: 'tavily', label: 'Tavily' },
];

const MODEL_OPTIONS: Record<string, { label: string; value: string }[]> = {
  anthropic: [
    { label: 'Claude Opus 4.6', value: 'claude-opus-4-20250415' },
    { label: 'Claude Sonnet 4.6', value: 'claude-sonnet-4-20250514' },
    { label: 'Claude Haiku 3.5', value: 'claude-haiku-4-5-20251001' },
  ],
  minimax: [
    { label: 'MiniMax M2.7', value: 'MiniMax-M2.7' },
    { label: 'MiniMax M2.7 Highspeed', value: 'MiniMax-M2.7-highspeed' },
    { label: 'MiniMax M2.5', value: 'MiniMax-M2.5' },
    { label: 'MiniMax M2.5 Highspeed', value: 'MiniMax-M2.5-highspeed' },
  ],
};

interface ConfigTabProps {
  agentId: string;
  agentName: string;
  onDirtyChange?: (dirty: boolean) => void;
}

export const ConfigTab = forwardRef<{ triggerSave: () => void }, ConfigTabProps>(function ConfigTab(
  { agentId, agentName, onDirtyChange },
  ref
) {
  const queryClient = useQueryClient();
  const [, setSaving] = useState(false);
  const [, setSaveError] = useState<string | null>(null);
  const [, setSaved] = useState(false);
  const [config, setConfig] = useState<AgentWorkspaceConfig | null>(null);
  const [, setIsDirty] = useState(false);

  // Expose triggerSave via ref
  useImperativeHandle(ref, () => ({
    triggerSave: () => handleSave(),
  }));

  function markDirty() {
    setIsDirty(true);
    onDirtyChange?.(true);
  }

  function markClean() {
    setIsDirty(false);
    onDirtyChange?.(false);
  }

  // Helper to update config and mark dirty
  function updateConfig(updates: Partial<AgentWorkspaceConfig>) {
    setConfig((prev) => (prev ? { ...prev, ...updates } : null));
    markDirty();
  }

  // API key state
  const [hasKey, setHasKey] = useState(false);
  const [keyInput, setKeyInput] = useState('');
  const [showKeyInput, setShowKeyInput] = useState(false);

  const { data: loadedConfig } = useQuery({
    queryKey: ['agent-config', agentId],
    queryFn: () => workspaceApi.getConfig(agentId),
  });

  useEffect(() => {
    if (loadedConfig) {
      setConfig(loadedConfig);
      // Check API key status for the provider
      llmApi
        .hasApiKey(loadedConfig.provider)
        .then(setHasKey)
        .catch(() => setHasKey(false));
    }
  }, [loadedConfig]);

  async function handleSave() {
    if (!config) return;
    setSaving(true);
    setSaveError(null);
    setSaved(false);
    try {
      await workspaceApi.updateConfig(agentId, config);
      queryClient.invalidateQueries({ queryKey: ['agent-config', agentId] });
      queryClient.invalidateQueries({ queryKey: ['agent-role-ids'] });
      setSaved(true);
      markClean();
      setTimeout(() => setSaved(false), 2000);
    } catch (err) {
      setSaveError(String(err));
    }
    setSaving(false);
  }

  async function handleSetApiKey() {
    if (!config || !keyInput.trim()) return;
    try {
      await llmApi.setApiKey(config.provider, keyInput.trim());
      setHasKey(true);
      setKeyInput('');
      setShowKeyInput(false);
    } catch (err) {
      console.error('Failed to set API key:', err);
    }
  }

  async function handleDeleteApiKey() {
    if (!config) return;
    if (!(await confirm('Remove API key?'))) return;
    try {
      await llmApi.deleteApiKey(config.provider);
      setHasKey(false);
    } catch (err) {
      console.error('Failed to delete API key:', err);
    }
  }

  function toggleTool(toolId: string) {
    if (!config) return;
    // If allowedTools is empty (meaning "all"), expand to explicit list first
    const currentTools = config.allowedTools.length === 0 ? [...ALL_TOOL_IDS] : config.allowedTools;
    let tools = currentTools.includes(toolId)
      ? currentTools.filter((t) => t !== toolId)
      : [...currentTools, toolId];
    // When all non-finish tools are enabled, normalize back to empty (means "all")
    if (ALL_TOOL_IDS.every((id) => tools.includes(id))) {
      tools = [];
    }
    updateConfig({ allowedTools: tools });
  }

  function isToolEnabled(toolId: string): boolean {
    if (!config) return false;
    return config.allowedTools.length === 0 || config.allowedTools.includes(toolId);
  }

  const allToolsEnabled = config ? config.allowedTools.length === 0 : false;

  function toggleAllTools(enableAll: boolean) {
    if (enableAll) {
      updateConfig({ allowedTools: [] });
    } else {
      // Disable all except finish (which is always on)
      updateConfig({ allowedTools: ['finish'] });
    }
  }

  function handleRoleChange(newRoleId: string) {
    updateConfig({
      roleId: newRoleId,
      roleSystemInstructions: getRoleSystemInstructions(newRoleId),
      allowedTools: getRoleDefaultTools(newRoleId),
    });
  }

  function isRoleDefaultsDirty(): boolean {
    if (!config?.roleId || config.roleId === DEFAULT_ROLE_ID) return false;
    const defaultTools = getRoleDefaultTools(config.roleId);
    const current = config.allowedTools;
    if (defaultTools.length === 0 && current.length === 0) return false;
    if (defaultTools.length !== current.length) return true;
    return !defaultTools.every((t) => current.includes(t));
  }

  if (!config) {
    return <div className="p-6 text-muted text-sm">Loading configuration...</div>;
  }

  const models = MODEL_OPTIONS[config.provider] ?? [];

  return (
    <div className="p-6 space-y-6 h-full overflow-y-auto">
      {/* Provider, Model & API Key — merged */}
      <section className="space-y-3">
        <h4 className="text-sm font-semibold text-white">Model</h4>
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="text-xs text-muted mb-1 block">Provider</label>
            <Select.Root
              value={config.provider}
              onValueChange={(value) => {
                updateConfig({ provider: value });
                llmApi
                  .hasApiKey(value)
                  .then(setHasKey)
                  .catch(() => setHasKey(false));
              }}
            >
              <Select.Trigger className="flex items-center justify-between w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent">
                <Select.Value />
                <Select.Icon>
                  <ChevronDown size={14} className="text-muted" />
                </Select.Icon>
              </Select.Trigger>
              <Select.Portal>
                <Select.Content className="rounded-lg bg-surface border border-edge shadow-xl overflow-hidden z-50">
                  <Select.Viewport className="p-1">
                    <Select.Item
                      value="anthropic"
                      className="px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-accent/20"
                    >
                      <Select.ItemText>Anthropic</Select.ItemText>
                    </Select.Item>
                    <Select.Item
                      value="minimax"
                      className="px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-accent/20"
                    >
                      <Select.ItemText>MiniMax</Select.ItemText>
                    </Select.Item>
                  </Select.Viewport>
                </Select.Content>
              </Select.Portal>
            </Select.Root>
          </div>
          <div>
            <label className="text-xs text-muted mb-1 block">Model</label>
            <Select.Root
              value={config.model}
              onValueChange={(value) => updateConfig({ model: value })}
            >
              <Select.Trigger className="flex items-center justify-between w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent">
                <Select.Value />
                <Select.Icon>
                  <ChevronDown size={14} className="text-muted" />
                </Select.Icon>
              </Select.Trigger>
              <Select.Portal>
                <Select.Content className="rounded-lg bg-surface border border-edge shadow-xl overflow-hidden z-50">
                  <Select.Viewport className="p-1">
                    {models.map((m) => (
                      <Select.Item
                        key={m.value}
                        value={m.value}
                        className="px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-accent/20"
                      >
                        <Select.ItemText>{m.label}</Select.ItemText>
                      </Select.Item>
                    ))}
                    {!models.find((m) => m.value === config.model) && (
                      <Select.Item
                        value={config.model}
                        className="px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-accent/20"
                      >
                        <Select.ItemText>{config.model}</Select.ItemText>
                      </Select.Item>
                    )}
                  </Select.Viewport>
                </Select.Content>
              </Select.Portal>
            </Select.Root>
          </div>
        </div>

        {/* Inline API key status */}
        <div className="rounded-lg border border-edge bg-background px-3 py-2">
          {hasKey ? (
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Check size={14} className="text-emerald-400" />
                <span className="text-sm text-emerald-400">
                  {config.provider} API key configured
                </span>
              </div>
              <button
                onClick={handleDeleteApiKey}
                className="flex items-center gap-1 px-2 py-1 rounded text-xs text-red-400 hover:bg-red-500/10"
              >
                <Trash2 size={11} /> Remove
              </button>
            </div>
          ) : showKeyInput ? (
            <div className="space-y-2">
              <input
                type="password"
                placeholder={`Enter ${config.provider} API key...`}
                value={keyInput}
                onChange={(e) => setKeyInput(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handleSetApiKey()}
                autoFocus
                className="w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm font-mono focus:outline-none focus:border-accent"
              />
              <div className="flex gap-2">
                <button
                  onClick={handleSetApiKey}
                  disabled={!keyInput.trim()}
                  className="px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-50 text-white text-xs font-medium"
                >
                  Save Key
                </button>
                <button
                  onClick={() => setShowKeyInput(false)}
                  className="px-3 py-1.5 rounded-lg text-muted hover:text-white text-xs"
                >
                  Cancel
                </button>
              </div>
            </div>
          ) : (
            <button
              onClick={() => setShowKeyInput(true)}
              className="flex items-center gap-2 text-secondary hover:text-white text-sm transition-colors"
            >
              <Key size={14} />
              Set {config.provider} API key
            </button>
          )}
        </div>
      </section>

      <AgentIdentitySection
        identity={config.identity}
        onChange={(identity) => updateConfig({ identity })}
        agentName={agentName}
        roleInstructions={config.roleSystemInstructions}
        showPreview
      />

      {/* Role */}
      <section className="space-y-3">
        <div className="flex items-center justify-between">
          <div>
            <h4 className="text-sm font-semibold text-white">Role</h4>
            <p className="text-xs text-muted mt-1">
              Pre-configures tools and injects role-specific instructions into the system prompt.
            </p>
          </div>
          {isRoleDefaultsDirty() && (
            <button
              onClick={() => {
                if (!config.roleId) return;
                updateConfig({
                  allowedTools: getRoleDefaultTools(config.roleId),
                  roleSystemInstructions: getRoleSystemInstructions(config.roleId),
                });
              }}
              className="shrink-0 px-2.5 py-1 rounded-lg border border-edge text-xs text-muted hover:text-white hover:border-edge-hover transition-colors"
            >
              Reset to defaults
            </button>
          )}
        </div>
        <RoleSelector selected={config.roleId} onSelect={handleRoleChange} mode="compact" />
      </section>

      {/* Behavior / Temperature */}
      <section className="space-y-3">
        <div>
          <h4 className="text-sm font-semibold text-white">Behavior</h4>
          <p className="text-xs text-muted mt-1">
            Controls how predictable or creative the agent's responses are. Lower values stick to
            the most likely answer, higher values introduce more variety.
          </p>
        </div>
        <div className="flex gap-2">
          {[
            { value: 0, label: 'Precise', desc: 'Consistent, factual, best for analysis' },
            { value: 0.3, label: 'Balanced', desc: 'Reliable with slight flexibility' },
            { value: 0.7, label: 'Creative', desc: 'Varied, exploratory responses' },
            { value: 1, label: 'Experimental', desc: 'Highly creative, less predictable' },
          ].map((preset) => {
            const selected = config.temperature === preset.value;
            return (
              <button
                key={preset.value}
                onClick={() => updateConfig({ temperature: preset.value })}
                className={`flex-1 flex flex-col items-center px-2 py-2.5 rounded-lg border text-center transition-colors ${
                  selected
                    ? 'border-accent bg-accent/10'
                    : 'border-edge bg-surface hover:border-edge-hover'
                }`}
              >
                <span
                  className={`text-sm font-medium ${selected ? 'text-accent-light' : 'text-white'}`}
                >
                  {preset.label}
                </span>
                <span className="text-[11px] text-muted mt-0.5 leading-tight">{preset.desc}</span>
              </button>
            );
          })}
          <div className="flex flex-col items-center gap-1">
            <label className="text-[11px] text-muted">Custom</label>
            <input
              type="number"
              min={0}
              max={2}
              step={0.05}
              value={config.temperature}
              onChange={(e) => {
                const v = parseFloat(e.target.value);
                if (!isNaN(v) && v >= 0 && v <= 2) updateConfig({ temperature: v });
              }}
              className="w-16 px-2 py-1.5 rounded-lg bg-background border border-edge text-white text-sm font-mono text-center focus:outline-none focus:border-accent"
            />
            <span className="text-[10px] text-muted">0 – 2</span>
          </div>
        </div>
      </section>

      {/* Advanced Settings — collapsed by default */}
      <CollapsibleSection
        title="Advanced Settings"
        description="Limits, context, web search, and tool permissions"
      >
        <div className="space-y-6">
          {/* Limits & Context */}
          <div className="space-y-3">
            <h5 className="text-xs font-semibold text-secondary uppercase tracking-wide">
              Limits & Context
            </h5>
            <div className="grid grid-cols-2 gap-3">
              <div>
                <label className="text-xs text-muted mb-1 block">Max Iterations</label>
                <input
                  type="number"
                  min={1}
                  max={100}
                  value={config.maxIterations}
                  onChange={(e) => updateConfig({ maxIterations: parseInt(e.target.value) || 25 })}
                  className="w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent"
                />
                <span className="text-[10px] text-muted mt-0.5 block">Default 25</span>
              </div>
              <div>
                <label className="text-xs text-muted mb-1 block">Max Total Tokens</label>
                <input
                  type="number"
                  min={1000}
                  step={10000}
                  value={config.maxTotalTokens}
                  onChange={(e) =>
                    updateConfig({ maxTotalTokens: parseInt(e.target.value) || 200000 })
                  }
                  className="w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent"
                />
                <span className="text-[10px] text-muted mt-0.5 block">Default 200k</span>
              </div>
            </div>
            <div className="grid grid-cols-3 gap-3">
              <div>
                <label className="text-xs text-muted mb-1 block">Compaction Threshold</label>
                <div className="flex items-center gap-2">
                  <input
                    type="number"
                    min={10}
                    max={95}
                    step={5}
                    value={Math.round((config.compactionThreshold ?? 0.65) * 100)}
                    onChange={(e) => {
                      const v = parseInt(e.target.value);
                      if (!isNaN(v) && v >= 10 && v <= 95)
                        updateConfig({ compactionThreshold: v / 100 });
                    }}
                    className="w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent"
                  />
                  <span className="text-xs text-muted shrink-0">%</span>
                </div>
              </div>
              <div>
                <label className="text-xs text-muted mb-1 block">Messages to Retain</label>
                <input
                  type="number"
                  min={2}
                  max={50}
                  value={config.compactionRetainCount ?? 12}
                  onChange={(e) =>
                    updateConfig({ compactionRetainCount: parseInt(e.target.value) || 12 })
                  }
                  className="w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent"
                />
              </div>
              <div>
                <label className="text-xs text-muted mb-1 block">Context Window Override</label>
                <input
                  type="number"
                  min={1000}
                  step={10000}
                  placeholder="Auto"
                  value={config.contextWindowOverride ?? ''}
                  onChange={(e) => {
                    const raw = e.target.value;
                    updateConfig({
                      contextWindowOverride: raw ? parseInt(raw) || undefined : undefined,
                    });
                  }}
                  className="w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent placeholder:text-border-hover"
                />
              </div>
            </div>
          </div>

          {/* Web Search */}
          <div className="space-y-3">
            <h5 className="text-xs font-semibold text-secondary uppercase tracking-wide">
              Web Search
            </h5>
            <div className="grid grid-cols-2 gap-3">
              <div>
                <label className="text-xs text-muted mb-1 block">Provider</label>
                <Select.Root
                  value={config.webSearchProvider}
                  onValueChange={(value) => updateConfig({ webSearchProvider: value })}
                >
                  <Select.Trigger className="flex items-center justify-between w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent">
                    <Select.Value />
                    <Select.Icon>
                      <ChevronDown size={14} className="text-muted" />
                    </Select.Icon>
                  </Select.Trigger>
                  <Select.Portal>
                    <Select.Content className="rounded-lg bg-surface border border-edge shadow-xl overflow-hidden z-50">
                      <Select.Viewport className="p-1">
                        {SEARCH_PROVIDERS.map((p) => (
                          <Select.Item
                            key={p.value}
                            value={p.value}
                            className="px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-accent/20"
                          >
                            <Select.ItemText>{p.label}</Select.ItemText>
                          </Select.Item>
                        ))}
                      </Select.Viewport>
                    </Select.Content>
                  </Select.Portal>
                </Select.Root>
              </div>
              <div>
                <label className="text-xs text-muted mb-1 block">API Key</label>
                <SearchKeyStatus provider={config.webSearchProvider} />
              </div>
            </div>
          </div>

          {/* Allowed Tools */}
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <h5 className="text-xs font-semibold text-secondary uppercase tracking-wide">
                Allowed Tools
              </h5>
              <div className="flex items-center gap-2">
                <span className="text-xs text-muted">All</span>
                <Switch.Root
                  checked={allToolsEnabled}
                  onCheckedChange={toggleAllTools}
                  className="w-9 h-5 rounded-full bg-edge data-[state=checked]:bg-emerald-500 transition-colors outline-none"
                >
                  <Switch.Thumb className="block w-4 h-4 rounded-full bg-white shadow translate-x-0.5 data-[state=checked]:translate-x-[18px] transition-transform" />
                </Switch.Root>
              </div>
            </div>

            <div className="flex flex-wrap gap-1.5">
              {TOOL_CATEGORIES.flatMap((category) =>
                category.tools.map((tool) => {
                  const enabled = isToolEnabled(tool.id);
                  return (
                    <button
                      key={tool.id}
                      onClick={() => toggleTool(tool.id)}
                      className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg border text-xs font-medium transition-colors ${
                        enabled
                          ? 'border-accent/40 bg-accent/10 text-accent-light hover:bg-accent/15'
                          : 'border-edge bg-surface text-muted hover:border-edge-hover hover:text-white'
                      }`}
                    >
                      <span
                        className={`w-1.5 h-1.5 rounded-full shrink-0 ${
                          enabled ? 'bg-emerald-400' : 'bg-edge-hover'
                        }`}
                      />
                      {tool.label}
                    </button>
                  );
                })
              )}
              {/* Finish — always on */}
              <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg border border-edge bg-surface text-xs text-muted opacity-50">
                <span className="w-1.5 h-1.5 rounded-full shrink-0 bg-emerald-400" />
                Finish
              </span>
            </div>
          </div>
        </div>
      </CollapsibleSection>

      {/* Memory — collapsed by default */}
      <CollapsibleSection
        title="Memory"
        description="Long-term memory across sessions"
      >
        <div className="space-y-4">
          {/* Enable / disable */}
          <div className="flex items-center justify-between">
            <div>
              <label className="text-xs text-white font-medium">Enable memory</label>
              <p className="text-[10px] text-muted mt-0.5">
                Inject relevant memories into context and extract new ones after sessions.
              </p>
            </div>
            <Switch.Root
              checked={config?.memoryEnabled ?? true}
              onCheckedChange={(v) => updateConfig({ memoryEnabled: v })}
              className="w-9 h-5 rounded-full bg-edge data-[state=checked]:bg-emerald-500 transition-colors outline-none shrink-0"
            >
              <Switch.Thumb className="block w-4 h-4 rounded-full bg-white shadow translate-x-0.5 data-[state=checked]:translate-x-[18px] transition-transform" />
            </Switch.Root>
          </div>

          {/* Staleness threshold */}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <label className="text-xs text-muted">Staleness threshold</label>
              <span className="text-xs text-white font-medium tabular-nums">
                {config?.memoryStalenessThresholdDays ?? 30}d
              </span>
            </div>
            <Slider.Root
              min={7}
              max={90}
              step={1}
              value={[config?.memoryStalenessThresholdDays ?? 30]}
              onValueChange={([v]) => updateConfig({ memoryStalenessThresholdDays: v })}
              disabled={!(config?.memoryEnabled ?? true)}
              className="relative flex items-center select-none touch-none w-full h-5"
            >
              <Slider.Track className="bg-edge relative grow rounded-full h-1">
                <Slider.Range className="absolute bg-accent rounded-full h-full" />
              </Slider.Track>
              <Slider.Thumb className="block w-4 h-4 bg-white rounded-full shadow border border-edge/50 hover:border-accent focus:outline-none focus:border-accent" />
            </Slider.Root>
            <p className="text-[10px] text-muted">
              Memories older than this many days are flagged as stale in context.
            </p>
          </div>
        </div>
      </CollapsibleSection>

      {/* Permissions — collapsed by default */}
      <CollapsibleSection
        title="Permissions"
        description="Control which tool actions require user approval"
      >
        <div className="space-y-4">
          {/* Permission Mode */}
          <div>
            <label className="text-xs text-muted block mb-1">Permission Mode</label>
            <div className="flex gap-2">
              {PERMISSION_MODES.map((mode) => (
                <button
                  key={mode.value}
                  onClick={() =>
                    updateConfig({
                      permissionMode: mode.value as AgentWorkspaceConfig['permissionMode'],
                    })
                  }
                  className={`px-3 py-1.5 rounded text-xs transition-colors ${
                    config?.permissionMode === mode.value
                      ? 'bg-accent/20 text-accent-hover border border-accent/40'
                      : 'bg-surface border border-edge text-secondary hover:border-edge-hover'
                  }`}
                  title={mode.description}
                >
                  {mode.label}
                </button>
              ))}
            </div>
            <p className="text-[10px] text-muted mt-1">
              {PERMISSION_MODES.find((m) => m.value === config?.permissionMode)?.description}
            </p>
          </div>

          {/* Saved Permission Rules */}
          <div>
            <label className="text-xs text-muted block mb-1">
              Saved Rules ({config?.permissionRules?.length ?? 0})
            </label>
            {!config?.permissionRules || config.permissionRules.length === 0 ? (
              <p className="text-[10px] text-muted italic">
                No saved rules. Click "Always Allow" or "Always Deny" on a permission prompt to
                create rules.
              </p>
            ) : (
              <div className="space-y-1">
                {config.permissionRules.map((rule) => (
                  <div
                    key={rule.id}
                    className="flex items-center gap-2 px-2 py-1.5 rounded bg-background/50 border border-edge/50 text-xs"
                  >
                    <span
                      className={`px-1.5 py-0.5 rounded text-[10px] font-medium ${
                        rule.decision === 'allow'
                          ? 'bg-emerald-500/10 text-emerald-400'
                          : 'bg-red-500/10 text-red-400'
                      }`}
                    >
                      {rule.decision}
                    </span>
                    <span className="text-warning font-mono">{rule.tool}</span>
                    <span className="text-muted font-mono truncate flex-1">{rule.pattern}</span>
                    <button
                      onClick={async () => {
                        await permissionsApi.deleteRule(agentId, rule.id);
                        updateConfig({
                          permissionRules: config.permissionRules.filter((r) => r.id !== rule.id),
                        });
                      }}
                      className="text-muted hover:text-red-400 transition-colors shrink-0"
                      title="Delete rule"
                    >
                      <Trash2 size={12} />
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </CollapsibleSection>
    </div>
  );
});

function SearchKeyStatus({ provider }: { provider: string }) {
  const [hasKey, setHasKey] = useState(false);
  const [keyInput, setKeyInput] = useState('');
  const [showInput, setShowInput] = useState(false);

  useEffect(() => {
    llmApi
      .hasApiKey(provider)
      .then(setHasKey)
      .catch(() => setHasKey(false));
  }, [provider]);

  async function handleSet() {
    if (!keyInput.trim()) return;
    try {
      await llmApi.setApiKey(provider, keyInput.trim());
      setHasKey(true);
      setKeyInput('');
      setShowInput(false);
    } catch (err) {
      console.error('Failed to set search API key:', err);
    }
  }

  if (hasKey) {
    return (
      <div className="flex items-center gap-2 h-[38px]">
        <Check size={14} className="text-emerald-400" />
        <span className="text-sm text-emerald-400">Configured</span>
        <button
          onClick={async () => {
            await llmApi.deleteApiKey(provider);
            setHasKey(false);
          }}
          className="ml-auto flex items-center gap-1 px-2 py-1 rounded text-xs text-red-400 hover:bg-red-500/10"
        >
          <Trash2 size={11} /> Remove
        </button>
      </div>
    );
  }

  if (showInput) {
    return (
      <div className="flex gap-2">
        <input
          type="password"
          placeholder={`${provider} API key...`}
          value={keyInput}
          onChange={(e) => setKeyInput(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && handleSet()}
          autoFocus
          className="flex-1 px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm font-mono focus:outline-none focus:border-accent"
        />
        <button
          onClick={handleSet}
          className="px-3 py-1.5 rounded-lg bg-accent text-white text-xs font-medium"
        >
          Save
        </button>
        <button onClick={() => setShowInput(false)} className="px-2 py-1.5 text-muted text-xs">
          Cancel
        </button>
      </div>
    );
  }

  return (
    <button
      onClick={() => setShowInput(true)}
      className="flex items-center gap-2 w-full px-3 py-2 rounded-lg border border-dashed border-edge-hover text-secondary hover:text-white hover:border-accent text-sm transition-colors"
    >
      <Key size={14} />
      Set {provider} API key
    </button>
  );
}
