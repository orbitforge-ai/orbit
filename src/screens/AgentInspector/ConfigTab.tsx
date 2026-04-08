import { useEffect, useState, useImperativeHandle, forwardRef } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Check, ChevronDown, FolderOpen, X, ExternalLink, AlertTriangle } from 'lucide-react';
import * as Select from '@radix-ui/react-select';
import * as Slider from '@radix-ui/react-slider';
import * as Switch from '@radix-ui/react-switch';

import { workspaceApi } from '../../api/workspace';
import { projectsApi } from '../../api/projects';
import { AgentWorkspaceConfig, Project } from '../../types';
import { CollapsibleSection } from '../../components/CollapsibleSection';
import { AgentIdentitySection } from './AgentIdentitySection';
import { MODEL_OPTIONS, LLM_PROVIDERS, DEFAULT_MODEL_BY_PROVIDER } from '../../constants/providers';
import { TOOL_CATEGORIES } from '../../constants/tools';
import { useApiKeyStatus } from '../../hooks/useApiKeyStatus';
import { useUiStore } from '../../store/uiStore';
import { useSettingsStore } from '../../store/settingsStore';


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

  const { navigate } = useUiStore();

  const { data: loadedConfig } = useQuery({
    queryKey: ['agent-config', agentId],
    queryFn: () => workspaceApi.getConfig(agentId),
  });

  useEffect(() => {
    if (loadedConfig) {
      setConfig(loadedConfig);
    }
  }, [loadedConfig]);

  const { data: hasKey = false } = useApiKeyStatus(config?.provider ?? '');

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

  function toggleDisabledTool(toolId: string) {
    if (!config) return;
    const current = config.disabledTools ?? [];
    const next = current.includes(toolId)
      ? current.filter((t) => t !== toolId)
      : [...current, toolId];
    updateConfig({ disabledTools: next });
  }

  function isToolDisabled(toolId: string): boolean {
    if (!config) return false;
    return (config.disabledTools ?? []).includes(toolId);
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
                const newModels = MODEL_OPTIONS[value] ?? [];
                const currentModelValid = newModels.some((m) => m.value === config.model);
                updateConfig({
                  provider: value,
                  ...(!currentModelValid && { model: DEFAULT_MODEL_BY_PROVIDER[value] ?? newModels[0]?.value ?? config.model }),
                });
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
                    {LLM_PROVIDERS.map((p) => (
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

        {/* API key status */}
        <div className="rounded-lg border border-edge bg-background px-3 py-2">
          {hasKey ? (
            <div className="flex items-center gap-2">
              <Check size={13} className="text-emerald-400" />
              <span className="text-xs text-emerald-400">API key configured in Settings</span>
            </div>
          ) : (
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <AlertTriangle size={13} className="text-amber-400" />
                <span className="text-xs text-amber-400">No API key for {config.provider}</span>
              </div>
              <button
                onClick={() => navigate('settings')}
                className="flex items-center gap-1 text-xs text-accent-hover hover:underline transition-colors"
              >
                Open Settings <ExternalLink size={11} />
              </button>
            </div>
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

          {/* Disabled Tools */}
          <div className="space-y-3">
            <div>
              <h5 className="text-xs font-semibold text-secondary uppercase tracking-wide">
                Disabled Tools
              </h5>
              <p className="text-[10px] text-muted mt-1">
                The global allow-list is managed in Settings. Toggle individual tools off for
                this agent only.
              </p>
            </div>

            <div className="flex flex-wrap gap-1.5">
              {TOOL_CATEGORIES.flatMap((category) =>
                category.tools.map((tool) => {
                  const disabled = isToolDisabled(tool.id);
                  return (
                    <button
                      key={tool.id}
                      onClick={() => toggleDisabledTool(tool.id)}
                      className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg border text-xs font-medium transition-colors ${
                        disabled
                          ? 'border-red-500/40 bg-red-500/10 text-red-300 hover:bg-red-500/15'
                          : 'border-edge bg-surface text-secondary hover:border-edge-hover hover:text-white'
                      }`}
                    >
                      <span
                        className={`w-1.5 h-1.5 rounded-full shrink-0 ${
                          disabled ? 'bg-red-400' : 'bg-emerald-400'
                        }`}
                      />
                      {tool.label}
                    </button>
                  );
                })
              )}
            </div>
          </div>
        </div>
      </CollapsibleSection>

      {/* Default channel — only visible when global channels exist */}
      <DefaultChannelSelect
        value={config.defaultChannelId}
        onChange={(id) => updateConfig({ defaultChannelId: id })}
      />

      {/* Memory — collapsed by default */}
      <CollapsibleSection title="Memory" description="Long-term memory across sessions">
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

      {/* Projects */}
      <AgentProjectsSection agentId={agentId} />
    </div>
  );
});

// ─── Default Channel Select ───────────────────────────────────────────────────

function DefaultChannelSelect({
  value,
  onChange,
}: {
  value: string | undefined;
  onChange: (id: string | undefined) => void;
}) {
  const channels = useSettingsStore((s) => s.settings.channels);
  const navigate = useUiStore((s) => s.navigate);

  if (channels.length === 0) {
    return (
      <section className="space-y-2">
        <h4 className="text-sm font-semibold text-white">Default Outbound Channel</h4>
        <div className="rounded-lg border border-dashed border-edge bg-background px-4 py-4 text-xs text-muted">
          No channels configured yet. Add one from{' '}
          <button
            onClick={() => navigate('settings')}
            className="text-accent-hover hover:underline"
          >
            Settings
          </button>{' '}
          to let this agent send messages without specifying a channel.
        </div>
      </section>
    );
  }

  const NONE_VALUE = '__none__';
  const selected = value ?? NONE_VALUE;

  return (
    <section className="space-y-2">
      <h4 className="text-sm font-semibold text-white">Default Outbound Channel</h4>
      <p className="text-xs text-muted">
        Used by the <span className="font-mono">message</span> tool when the agent calls it
        without specifying a channel.
      </p>
      <Select.Root
        value={selected}
        onValueChange={(v) => onChange(v === NONE_VALUE ? undefined : v)}
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
                value={NONE_VALUE}
                className="px-3 py-2 text-sm text-muted rounded-md outline-none cursor-pointer data-[highlighted]:bg-accent/20"
              >
                <Select.ItemText>No default</Select.ItemText>
              </Select.Item>
              {channels.map((channel) => (
                <Select.Item
                  key={channel.id}
                  value={channel.id}
                  className="px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-accent/20"
                >
                  <Select.ItemText>
                    {channel.name}
                    {!channel.enabled && ' (disabled)'}
                  </Select.ItemText>
                </Select.Item>
              ))}
            </Select.Viewport>
          </Select.Content>
        </Select.Portal>
      </Select.Root>
    </section>
  );
}

// ─── Agent Projects Section ───────────────────────────────────────────────────

function AgentProjectsSection({ agentId }: { agentId: string }) {
  const queryClient = useQueryClient();
  const [adding, setAdding] = useState(false);

  const { data: agentProjects = [] } = useQuery<Project[]>({
    queryKey: ['agent-projects', agentId],
    queryFn: () => projectsApi.listAgentProjects(agentId),
  });

  const { data: allProjects = [] } = useQuery<Project[]>({
    queryKey: ['projects'],
    queryFn: projectsApi.list,
    enabled: adding,
  });

  const memberIds = new Set(agentProjects.map((p) => p.id));
  const addableProjects = allProjects.filter((p) => !memberIds.has(p.id));

  async function handleAdd(projectId: string) {
    await projectsApi.addAgent(projectId, agentId, agentProjects.length === 0);
    queryClient.invalidateQueries({ queryKey: ['agent-projects', agentId] });
    queryClient.invalidateQueries({ queryKey: ['project-agents', projectId] });
    setAdding(false);
  }

  async function handleRemove(projectId: string) {
    await projectsApi.removeAgent(projectId, agentId);
    queryClient.invalidateQueries({ queryKey: ['agent-projects', agentId] });
    queryClient.invalidateQueries({ queryKey: ['project-agents', projectId] });
  }

  return (
    <CollapsibleSection title="Projects" description="Projects this agent is assigned to">
      <div className="space-y-3">
        {agentProjects.length === 0 ? (
          <p className="text-xs text-muted italic">Not assigned to any projects.</p>
        ) : (
          <ul className="space-y-2">
            {agentProjects.map((project) => (
              <li
                key={project.id}
                className="flex items-center gap-3 px-3 py-2 rounded-lg border border-edge bg-panel"
              >
                <FolderOpen size={13} className="text-muted shrink-0" />
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium text-white truncate">{project.name}</p>
                  {project.description && (
                    <p className="text-xs text-muted truncate">{project.description}</p>
                  )}
                </div>
                <button
                  onClick={() => handleRemove(project.id)}
                  className="p-1 rounded text-muted hover:text-red-400 hover:bg-red-400/10 transition-colors"
                  title="Remove from project"
                >
                  <X size={12} />
                </button>
              </li>
            ))}
          </ul>
        )}

        {adding ? (
          <div className="rounded-lg border border-edge bg-surface p-3 space-y-2">
            <p className="text-xs text-muted font-medium">Add to project:</p>
            {addableProjects.length === 0 ? (
              <p className="text-xs text-muted italic">All projects already assigned.</p>
            ) : (
              addableProjects.map((p) => (
                <button
                  key={p.id}
                  onClick={() => handleAdd(p.id)}
                  className="w-full flex items-center gap-2 px-3 py-2 rounded-lg border border-edge bg-panel hover:border-accent hover:bg-accent/10 transition-colors text-left"
                >
                  <FolderOpen size={13} className="text-muted shrink-0" />
                  <div className="flex-1 min-w-0">
                    <p className="text-sm font-medium text-white">{p.name}</p>
                    {p.description && (
                      <p className="text-xs text-muted truncate">{p.description}</p>
                    )}
                  </div>
                </button>
              ))
            )}
            <button
              onClick={() => setAdding(false)}
              className="text-xs text-muted hover:text-white transition-colors"
            >
              Cancel
            </button>
          </div>
        ) : (
          <button
            onClick={() => setAdding(true)}
            className="text-xs text-accent-hover hover:underline transition-colors"
          >
            + Add to project
          </button>
        )}
      </div>
    </CollapsibleSection>
  );
}
