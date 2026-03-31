import { useEffect, useState, useImperativeHandle, forwardRef } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Key, Trash2, Check, ChevronDown } from 'lucide-react';
import * as Select from '@radix-ui/react-select';
import * as Checkbox from '@radix-ui/react-checkbox';

import { workspaceApi } from '../../api/workspace';
import { llmApi } from '../../api/llm';
import { AgentWorkspaceConfig } from '../../types';
import { confirm } from '@tauri-apps/plugin-dialog';

const AVAILABLE_TOOLS = [
  { id: 'shell_command', label: 'Shell Commands' },
  { id: 'read_file', label: 'Read Files' },
  { id: 'write_file', label: 'Write Files' },
  { id: 'list_files', label: 'List Files' },
  { id: 'web_search', label: 'Web Search' },
  { id: 'activate_skill', label: 'Activate Skill' },
  { id: 'finish', label: 'Finish (always enabled)' },
];

const SEARCH_PROVIDERS = [
  { value: 'brave', label: 'Brave Search' },
  { value: 'tavily', label: 'Tavily' },
];

const MODEL_OPTIONS: Record<string, { label: string; value: string }[]> = {
  anthropic: [
    { label: 'Claude Sonnet 4', value: 'claude-sonnet-4-20250514' },
    { label: 'Claude Haiku 3.5', value: 'claude-haiku-4-5-20251001' },
  ],
  minimax: [
    { label: 'MiniMax M2.7', value: 'MiniMax-M2.7' },
    { label: 'MiniMax M2.7 Highspeed', value: 'MiniMax-M2.7-highspeed' },
    { label: 'MiniMax M2.5', value: 'MiniMax-M2.5' },
    { label: 'MiniMax M2.5 Highspeed', value: 'MiniMax-M2.5-highspeed' },
    { label: 'MiniMax M2.1', value: 'MiniMax-M2.1' },
    { label: 'MiniMax M2.1 Highspeed', value: 'MiniMax-M2.1-highspeed' },
    { label: 'MiniMax M2', value: 'MiniMax-M2' },
  ],
};

interface ConfigTabProps {
  agentId: string;
  onDirtyChange?: (dirty: boolean) => void;
}

export const ConfigTab = forwardRef<{ triggerSave: () => void }, ConfigTabProps>(function ConfigTab(
  { agentId, onDirtyChange },
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
    const tools = config.allowedTools.includes(toolId)
      ? config.allowedTools.filter((t) => t !== toolId)
      : [...config.allowedTools, toolId];
    updateConfig({ allowedTools: tools });
  }

  if (!config) {
    return <div className="p-6 text-muted text-sm">Loading configuration...</div>;
  }

  const models = MODEL_OPTIONS[config.provider] ?? [];

  return (
    <div className="p-6 space-y-6 h-full overflow-y-auto">
      {/* Provider & Model */}
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
      </section>

      {/* API Key */}
      <section className="space-y-3">
        <h4 className="text-sm font-semibold text-white">API Key</h4>
        <div className="rounded-xl border border-edge bg-surface p-4">
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
              className="flex items-center gap-2 px-3 py-2 rounded-lg border border-dashed border-edge-hover text-secondary hover:text-white hover:border-accent text-sm w-full transition-colors"
            >
              <Key size={14} />
              Set {config.provider} API key
            </button>
          )}
        </div>
      </section>

      {/* Temperature Presets */}
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

      {/* Limits */}
      <section className="space-y-3">
        <h4 className="text-sm font-semibold text-white">Limits</h4>
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
          </div>
          <div>
            <label className="text-xs text-muted mb-1 block">Max Total Tokens</label>
            <input
              type="number"
              min={1000}
              step={10000}
              value={config.maxTotalTokens}
              onChange={(e) => updateConfig({ maxTotalTokens: parseInt(e.target.value) || 200000 })}
              className="w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent"
            />
          </div>
        </div>
      </section>

      {/* Context Management */}
      <section className="space-y-3">
        <div>
          <h4 className="text-sm font-semibold text-white">Context Management</h4>
          <p className="text-xs text-muted mt-1">
            Controls automatic conversation compaction when the context window fills up.
          </p>
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
      </section>

      {/* Web Search Provider */}
      <section className="space-y-3">
        <div>
          <h4 className="text-sm font-semibold text-white">Web Search</h4>
          <p className="text-xs text-muted mt-1">
            Search provider used by the web_search tool. Requires an API key for the selected
            provider.
          </p>
        </div>
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
      </section>

      {/* Tools */}
      <section className="space-y-3">
        <h4 className="text-sm font-semibold text-white">Allowed Tools</h4>
        <div className="space-y-2">
          {AVAILABLE_TOOLS.map((tool) => {
            const isFinish = tool.id === 'finish';
            const checked = isFinish || config.allowedTools.includes(tool.id);
            return (
              <div
                key={tool.id}
                onClick={() => !isFinish && toggleTool(tool.id)}
                className={`flex items-center gap-3 px-3 py-2 rounded-lg border transition-colors cursor-pointer ${
                  checked ? 'border-accent/30 bg-accent/5' : 'border-edge bg-surface'
                } ${isFinish ? 'opacity-60 cursor-not-allowed' : ''}`}
              >
                <Checkbox.Root
                  checked={checked}
                  disabled={isFinish}
                  onCheckedChange={() => !isFinish && toggleTool(tool.id)}
                  className="flex items-center justify-center w-4 h-4 rounded border border-edge-hover bg-background data-[state=checked]:bg-accent data-[state=checked]:border-accent"
                >
                  <Checkbox.Indicator>
                    <Check size={10} className="text-white" />
                  </Checkbox.Indicator>
                </Checkbox.Root>
                <span className="text-sm text-white">{tool.label}</span>
                <span className="text-xs text-muted font-mono">{tool.id}</span>
              </div>
            );
          })}
        </div>
      </section>
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
