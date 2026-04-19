import { useEffect, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import * as Switch from '@radix-ui/react-switch';
import * as Select from '@radix-ui/react-select';
import {
  AlertTriangle,
  Check,
  ChevronDown,
  Key,
  Plus,
  Terminal,
  Trash2,
  X,
} from 'lucide-react';
import { confirm } from '@tauri-apps/plugin-dialog';
import { llmApi, ProviderStatus } from '../../api/llm';
import { useApiKeyStatus, useInvalidateApiKeys } from '../../hooks/useApiKeyStatus';
import {
  IMAGE_GENERATION_PROVIDERS,
  isCliProvider,
  LLM_PROVIDERS,
  SEARCH_PROVIDERS,
} from '../../constants/providers';
import { TOOL_CATEGORIES, TOOL_LABEL_BY_ID } from '../../constants/tools';
import { useSettingsStore } from '../../store/settingsStore';
import {
  AgentDefaults,
  ChannelConfig,
  ChannelType,
  PermissionRule,
} from '../../types';

function CliProviderRow({ provider, label }: { provider: string; label: string }) {
  const { data: status } = useQuery<ProviderStatus>({
    queryKey: ['providerStatus', provider],
    queryFn: () => llmApi.getProviderStatus(provider),
    staleTime: 10_000,
  });

  const ready = status?.ready ?? false;

  return (
    <div className="rounded-lg border border-edge bg-background px-4 py-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Terminal size={13} className="text-secondary" />
          <span className="text-sm font-medium text-white">{label}</span>
          <span className="rounded bg-surface px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-muted">
            local CLI
          </span>
        </div>
        {ready ? (
          <div className="flex items-center gap-1.5">
            <Check size={13} className="text-emerald-400" />
            <span className="text-xs text-emerald-400">Installed</span>
          </div>
        ) : (
          <div className="flex items-center gap-1.5">
            <AlertTriangle size={13} className="text-amber-400" />
            <span className="text-xs text-amber-400">Not found</span>
          </div>
        )}
      </div>
      {status?.binary_path && (
        <p className="mt-2 truncate font-mono text-[11px] text-muted">{status.binary_path}</p>
      )}
      {status?.message && (
        <p className="mt-2 text-xs text-amber-300/80">{status.message}</p>
      )}
      <p className="mt-2 text-[11px] text-muted">
        Runs through the local CLI. Orbit still owns tool execution and permissions through an
        embedded MCP bridge.
      </p>
    </div>
  );
}

function ProviderKeyRow({ provider, label }: { provider: string; label: string }) {
  if (isCliProvider(provider)) {
    return <CliProviderRow provider={provider} label={label} />;
  }
  return <ApiKeyProviderRow provider={provider} label={label} />;
}

function ApiKeyProviderRow({ provider, label }: { provider: string; label: string }) {
  const { data: hasKey = false } = useApiKeyStatus(provider);
  const invalidate = useInvalidateApiKeys();
  const [keyInput, setKeyInput] = useState('');
  const [editing, setEditing] = useState(false);

  async function handleSave() {
    if (!keyInput.trim()) return;
    try {
      await llmApi.setApiKey(provider, keyInput.trim());
      invalidate();
      setKeyInput('');
      setEditing(false);
    } catch (err) {
      console.error(`Failed to set ${provider} API key:`, err);
    }
  }

  async function handleRemove() {
    if (!(await confirm(`Remove ${label} API key?`))) return;
    try {
      await llmApi.deleteApiKey(provider);
      invalidate();
    } catch (err) {
      console.error(`Failed to delete ${provider} API key:`, err);
    }
  }

  return (
    <div className="rounded-lg border border-edge bg-background px-4 py-3">
      <div className="flex items-center justify-between">
        <span className="text-sm font-medium text-white">{label}</span>
        {hasKey ? (
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-1.5">
              <Check size={13} className="text-emerald-400" />
              <span className="text-xs text-emerald-400">Configured</span>
            </div>
            <button
              onClick={handleRemove}
              className="flex items-center gap-1 px-2 py-1 rounded text-xs text-red-400 hover:bg-red-500/10 transition-colors"
            >
              <Trash2 size={11} /> Remove
            </button>
          </div>
        ) : !editing ? (
          <button
            onClick={() => setEditing(true)}
            className="flex items-center gap-1.5 text-xs text-secondary hover:text-white transition-colors"
          >
            <Key size={12} />
            Add key
          </button>
        ) : null}
      </div>
      {editing && !hasKey && (
        <div className="mt-3 flex gap-2">
          <input
            type="password"
            placeholder={`Enter ${label} API key...`}
            value={keyInput}
            onChange={(e) => setKeyInput(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handleSave()}
            autoFocus
            className="flex-1 px-3 py-2 rounded-lg bg-surface border border-edge text-white text-sm font-mono focus:outline-none focus:border-accent"
          />
          <button
            onClick={handleSave}
            disabled={!keyInput.trim()}
            className="px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-50 text-white text-xs font-medium transition-colors"
          >
            Save
          </button>
          <button
            onClick={() => { setEditing(false); setKeyInput(''); }}
            className="px-3 py-1.5 rounded-lg text-muted hover:text-white text-xs transition-colors"
          >
            Cancel
          </button>
        </div>
      )}
    </div>
  );
}

// ─── Channels ────────────────────────────────────────────────────────────────

const CHANNEL_TYPE_OPTIONS: { value: ChannelType; label: string; hint: string }[] = [
  { value: 'slack', label: 'Slack', hint: 'Incoming webhook URL from a Slack app' },
  { value: 'discord', label: 'Discord', hint: 'Channel webhook URL from Discord' },
  { value: 'webhook', label: 'Generic webhook', hint: 'Any endpoint that accepts JSON POST' },
];

function generateChannelId() {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return crypto.randomUUID();
  }
  return `ch_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
}

function ChannelsSection() {
  const channels = useSettingsStore((s) => s.settings.channels);
  const upsertChannel = useSettingsStore((s) => s.upsertChannel);
  const removeChannel = useSettingsStore((s) => s.removeChannel);

  const [draft, setDraft] = useState<ChannelConfig | null>(null);

  function startAdd() {
    setDraft({
      id: generateChannelId(),
      name: '',
      type: 'slack',
      webhookUrl: '',
      enabled: true,
    });
  }

  async function handleSave() {
    if (!draft) return;
    if (!draft.name.trim()) return;
    if (!draft.webhookUrl.trim()) return;
    try {
      await upsertChannel({ ...draft, name: draft.name.trim(), webhookUrl: draft.webhookUrl.trim() });
      setDraft(null);
    } catch (err) {
      console.error('Failed to save channel:', err);
    }
  }

  async function handleDelete(channel: ChannelConfig) {
    if (!(await confirm(`Delete channel "${channel.name}"?`))) return;
    try {
      await removeChannel(channel.id);
    } catch (err) {
      console.error('Failed to delete channel:', err);
    }
  }

  async function handleToggle(channel: ChannelConfig, enabled: boolean) {
    try {
      await upsertChannel({ ...channel, enabled });
    } catch (err) {
      console.error('Failed to toggle channel:', err);
    }
  }

  return (
    <section className="space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold text-white">External Channels</h3>
        {!draft && (
          <button
            onClick={startAdd}
            className="flex items-center gap-1.5 rounded-md border border-edge bg-surface px-2.5 py-1 text-xs text-secondary transition-colors hover:bg-panel hover:text-white"
          >
            <Plus size={12} />
            Add channel
          </button>
        )}
      </div>
      <p className="text-xs text-muted">
        Slack, Discord, and generic webhooks. Any agent can send to these channels via the{' '}
        <span className="font-mono text-secondary">message</span> tool.
      </p>

      {channels.length === 0 && !draft && (
        <div className="rounded-lg border border-dashed border-edge bg-background px-4 py-6 text-center text-xs text-muted">
          No channels configured yet.
        </div>
      )}

      <div className="space-y-2">
        {channels.map((channel) => (
          <div
            key={channel.id}
            className="rounded-lg border border-edge bg-background px-4 py-3"
          >
            <div className="flex items-center justify-between gap-3">
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <span className="text-sm font-medium text-white">{channel.name}</span>
                  <span className="rounded bg-surface px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-muted">
                    {channel.type}
                  </span>
                </div>
                <p className="mt-1 truncate font-mono text-xs text-muted">
                  {channel.webhookUrl}
                </p>
              </div>
              <div className="flex items-center gap-3">
                <Switch.Root
                  checked={channel.enabled}
                  onCheckedChange={(v) => handleToggle(channel, v)}
                  className="w-9 h-5 rounded-full bg-edge data-[state=checked]:bg-accent transition-colors outline-none shrink-0"
                  aria-label={`Enable ${channel.name}`}
                >
                  <Switch.Thumb className="block w-4 h-4 rounded-full bg-white shadow translate-x-0.5 data-[state=checked]:translate-x-[18px] transition-transform" />
                </Switch.Root>
                <button
                  onClick={() => handleDelete(channel)}
                  className="flex items-center gap-1 px-2 py-1 rounded text-xs text-red-400 hover:bg-red-500/10 transition-colors"
                >
                  <Trash2 size={11} />
                </button>
              </div>
            </div>
          </div>
        ))}
      </div>

      {draft && (
        <div className="space-y-3 rounded-lg border border-accent/60 bg-background px-4 py-3">
          <div className="grid grid-cols-2 gap-3">
            <label className="flex flex-col gap-1">
              <span className="text-xs text-muted">Name</span>
              <input
                value={draft.name}
                onChange={(e) => setDraft({ ...draft, name: e.target.value })}
                placeholder="e.g. #ops-alerts"
                className="rounded-md bg-surface border border-edge px-2.5 py-1.5 text-sm text-white focus:outline-none focus:border-accent"
              />
            </label>
            <label className="flex flex-col gap-1">
              <span className="text-xs text-muted">Type</span>
              <Select.Root
                value={draft.type}
                onValueChange={(v) => setDraft({ ...draft, type: v as ChannelType })}
              >
                <Select.Trigger className="flex items-center justify-between rounded-md bg-surface border border-edge px-2.5 py-1.5 text-sm text-white focus:outline-none focus:border-accent">
                  <Select.Value />
                  <Select.Icon>
                    <ChevronDown size={14} />
                  </Select.Icon>
                </Select.Trigger>
                <Select.Portal>
                  <Select.Content
                    position="popper"
                    sideOffset={4}
                    className="z-50 min-w-[var(--radix-select-trigger-width)] rounded-md border border-edge bg-surface p-1 shadow-lg"
                  >
                    <Select.Viewport>
                      {CHANNEL_TYPE_OPTIONS.map((opt) => (
                        <Select.Item
                          key={opt.value}
                          value={opt.value}
                          className="cursor-pointer rounded px-2 py-1 text-sm text-white data-[highlighted]:bg-panel data-[highlighted]:outline-none"
                        >
                          <Select.ItemText>{opt.label}</Select.ItemText>
                        </Select.Item>
                      ))}
                    </Select.Viewport>
                  </Select.Content>
                </Select.Portal>
              </Select.Root>
            </label>
          </div>
          <label className="flex flex-col gap-1">
            <span className="text-xs text-muted">Webhook URL</span>
            <input
              value={draft.webhookUrl}
              onChange={(e) => setDraft({ ...draft, webhookUrl: e.target.value })}
              placeholder="https://hooks.slack.com/services/..."
              className="rounded-md bg-surface border border-edge px-2.5 py-1.5 text-sm text-white font-mono focus:outline-none focus:border-accent"
            />
            <span className="text-[11px] text-muted">
              {CHANNEL_TYPE_OPTIONS.find((o) => o.value === draft.type)?.hint}
            </span>
          </label>
          <div className="flex justify-end gap-2">
            <button
              onClick={() => setDraft(null)}
              className="rounded-md px-3 py-1.5 text-xs text-muted hover:text-white transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={handleSave}
              disabled={!draft.name.trim() || !draft.webhookUrl.trim()}
              className="rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-accent-hover disabled:opacity-50"
            >
              Save channel
            </button>
          </div>
        </div>
      )}
    </section>
  );
}

// ─── Shared agent defaults ───────────────────────────────────────────────────

const PERMISSION_MODE_OPTIONS: {
  value: AgentDefaults['permissionMode'];
  label: string;
  description: string;
}[] = [
  { value: 'normal', label: 'Normal', description: 'Prompt for writes/exec, auto-allow reads' },
  { value: 'strict', label: 'Strict', description: 'Prompt for all non-read operations' },
  {
    value: 'permissive',
    label: 'Permissive',
    description: 'Auto-allow everything (advanced users)',
  },
];

function AgentDefaultsSection() {
  const agentDefaults = useSettingsStore((s) => s.settings.agentDefaults);
  const updateAgentDefaults = useSettingsStore((s) => s.updateAgentDefaults);

  // An empty list in global settings means "all default tools". The UI
  // surfaces that as "every tool allowed" until the user clicks a single tool
  // to start customizing.
  const allToolsEnabled = agentDefaults.allowedTools.length === 0;

  const allowedSet = useMemo(
    () => new Set(agentDefaults.allowedTools),
    [agentDefaults.allowedTools],
  );

  async function toggleTool(toolId: string, on: boolean) {
    // First interaction expands the implicit "all" list into an explicit one
    // that excludes only the tool being turned off.
    if (allToolsEnabled) {
      if (on) return; // already on
      const explicit = TOOL_CATEGORIES.flatMap((c) => c.tools.map((t) => t.id)).filter(
        (id) => id !== toolId,
      );
      await updateAgentDefaults({ allowedTools: explicit });
      return;
    }
    const next = on
      ? [...agentDefaults.allowedTools, toolId]
      : agentDefaults.allowedTools.filter((id) => id !== toolId);
    await updateAgentDefaults({ allowedTools: next });
  }

  async function resetToAll() {
    await updateAgentDefaults({ allowedTools: [] });
  }

  return (
    <section className="space-y-3">
      <h3 className="text-sm font-semibold text-white">Agent Defaults</h3>
      <p className="text-xs text-muted">
        These defaults apply to every agent at runtime. Individual agents can opt out of
        specific tools via their own config.
      </p>

      <div className="rounded-lg border border-edge bg-background px-4 py-3">
        <div className="flex items-center justify-between gap-4">
          <div>
            <label className="text-sm font-medium text-white">Permission mode</label>
            <p className="text-xs text-muted mt-1">
              Controls how destructive tool calls are gated.
            </p>
          </div>
          <Select.Root
            value={agentDefaults.permissionMode}
            onValueChange={(v) =>
              updateAgentDefaults({ permissionMode: v as AgentDefaults['permissionMode'] })
            }
          >
            <Select.Trigger className="flex items-center justify-between gap-2 rounded-md bg-surface border border-edge px-2.5 py-1.5 text-sm text-white min-w-[130px] focus:outline-none focus:border-accent">
              <Select.Value />
              <Select.Icon>
                <ChevronDown size={14} />
              </Select.Icon>
            </Select.Trigger>
            <Select.Portal>
              <Select.Content
                position="popper"
                sideOffset={4}
                className="z-50 rounded-md border border-edge bg-surface p-1 shadow-lg"
              >
                <Select.Viewport>
                  {PERMISSION_MODE_OPTIONS.map((opt) => (
                    <Select.Item
                      key={opt.value}
                      value={opt.value}
                      className="cursor-pointer rounded px-2 py-1 text-sm text-white data-[highlighted]:bg-panel data-[highlighted]:outline-none"
                    >
                      <Select.ItemText>{opt.label}</Select.ItemText>
                      <div className="text-[11px] text-muted">{opt.description}</div>
                    </Select.Item>
                  ))}
                </Select.Viewport>
              </Select.Content>
            </Select.Portal>
          </Select.Root>
        </div>
      </div>

      <div className="rounded-lg border border-edge bg-background px-4 py-3">
        <div className="flex items-center justify-between gap-4">
          <div>
            <label className="text-sm font-medium text-white">Web search provider</label>
            <p className="text-xs text-muted mt-1">
              Provider used by the <span className="font-mono">web_search</span> tool.
            </p>
          </div>
          <Select.Root
            value={agentDefaults.webSearchProvider}
            onValueChange={(v) => updateAgentDefaults({ webSearchProvider: v })}
          >
            <Select.Trigger className="flex items-center justify-between gap-2 rounded-md bg-surface border border-edge px-2.5 py-1.5 text-sm text-white min-w-[130px] focus:outline-none focus:border-accent">
              <Select.Value />
              <Select.Icon>
                <ChevronDown size={14} />
              </Select.Icon>
            </Select.Trigger>
            <Select.Portal>
              <Select.Content
                position="popper"
                sideOffset={4}
                className="z-50 rounded-md border border-edge bg-surface p-1 shadow-lg"
              >
                <Select.Viewport>
                  {SEARCH_PROVIDERS.map((opt) => (
                    <Select.Item
                      key={opt.value}
                      value={opt.value}
                      className="cursor-pointer rounded px-2 py-1 text-sm text-white data-[highlighted]:bg-panel data-[highlighted]:outline-none"
                    >
                      <Select.ItemText>{opt.label}</Select.ItemText>
                    </Select.Item>
                  ))}
                </Select.Viewport>
              </Select.Content>
            </Select.Portal>
          </Select.Root>
        </div>
      </div>

      <div className="rounded-lg border border-edge bg-background px-4 py-3 space-y-3">
        <div className="flex items-center justify-between gap-4">
          <div>
            <label className="text-sm font-medium text-white">Allowed tools</label>
            <p className="text-xs text-muted mt-1">
              Tools available to every agent by default. Per-agent <span className="font-mono">disabledTools</span>{' '}
              further restricts this set.
            </p>
          </div>
          {!allToolsEnabled && (
            <button
              onClick={resetToAll}
              className="text-xs text-secondary hover:text-white transition-colors"
            >
              Reset to all
            </button>
          )}
        </div>

        <div className="space-y-2">
          {TOOL_CATEGORIES.map((category) => (
            <div key={category.label}>
              <div className="text-[11px] uppercase tracking-wide text-muted mb-1">
                {category.label}
              </div>
              <div className="flex flex-wrap gap-1.5">
                {category.tools.map((tool) => {
                  const enabled = allToolsEnabled || allowedSet.has(tool.id);
                  return (
                    <button
                      key={tool.id}
                      onClick={() => toggleTool(tool.id, !enabled)}
                      className={`rounded-full border px-2.5 py-1 text-xs transition-colors ${
                        enabled
                          ? 'border-accent/60 bg-accent/10 text-white'
                          : 'border-edge bg-background text-muted hover:text-secondary'
                      }`}
                    >
                      {tool.label}
                    </button>
                  );
                })}
              </div>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

// ─── Permission rules ────────────────────────────────────────────────────────

function PermissionRulesSection() {
  const rules = useSettingsStore((s) => s.settings.agentDefaults.permissionRules);
  const removeRule = useSettingsStore((s) => s.removePermissionRule);

  async function handleDelete(rule: PermissionRule) {
    if (!(await confirm(`Delete rule for "${rule.tool}"?`))) return;
    try {
      await removeRule(rule.id);
    } catch (err) {
      console.error('Failed to delete permission rule:', err);
    }
  }

  return (
    <section className="space-y-3">
      <h3 className="text-sm font-semibold text-white">Permission Rules</h3>
      <p className="text-xs text-muted">
        Persisted allow/deny rules. New rules are added by clicking{' '}
        <span className="text-secondary">Always allow</span> on a permission prompt.
      </p>

      {rules.length === 0 ? (
        <div className="rounded-lg border border-dashed border-edge bg-background px-4 py-6 text-center text-xs text-muted">
          No permission rules yet.
        </div>
      ) : (
        <div className="space-y-2">
          {rules.map((rule) => (
            <div
              key={rule.id}
              className="rounded-lg border border-edge bg-background px-4 py-3"
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <span
                      className={`rounded px-1.5 py-0.5 text-[10px] uppercase tracking-wide ${
                        rule.decision === 'allow'
                          ? 'bg-emerald-500/20 text-emerald-300'
                          : 'bg-red-500/20 text-red-300'
                      }`}
                    >
                      {rule.decision}
                    </span>
                    <span className="text-sm font-medium text-white">{rule.tool}</span>
                  </div>
                  <p className="mt-1 font-mono text-xs text-muted break-all">{rule.pattern}</p>
                  {rule.description && (
                    <p className="mt-1 text-xs text-muted">{rule.description}</p>
                  )}
                </div>
                <button
                  onClick={() => handleDelete(rule)}
                  className="flex items-center gap-1 px-2 py-1 rounded text-xs text-red-400 hover:bg-red-500/10 transition-colors"
                >
                  <Trash2 size={11} />
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

// ─── Settings screen shell ───────────────────────────────────────────────────

interface SettingsProps {
  onClose?: () => void;
}

export function Settings({ onClose }: SettingsProps = {}) {
  const showAgentThoughts = useSettingsStore((s) => s.showAgentThoughts);
  const showVerboseToolDetails = useSettingsStore((s) => s.showVerboseToolDetails);
  const setShowAgentThoughts = useSettingsStore((s) => s.setShowAgentThoughts);
  const setShowVerboseToolDetails = useSettingsStore((s) => s.setShowVerboseToolDetails);
  const loaded = useSettingsStore((s) => s.loaded);
  const loadError = useSettingsStore((s) => s.error);

  const handleClose = () => onClose?.();

  useEffect(() => {
    if (!onClose) return;

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') handleClose();
    }

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [onClose]);

  // The unused-var noise `TOOL_LABEL_BY_ID` is imported for co-location with
  // other UI that might render a tool label later — keep it in the module so
  // dead-code elimination doesn't drop the symbol from the bundle.
  void TOOL_LABEL_BY_ID;

  return (
    <div
      className={onClose ? 'absolute inset-0 z-40 bg-black/60 backdrop-blur-sm' : 'h-full'}
      onClick={handleClose}
    >
      <div className="h-full overflow-y-auto">
        <div
          className="max-w-2xl mx-auto p-8 space-y-8"
          onClick={(event) => event.stopPropagation()}
        >
          <div className="flex items-start justify-between gap-4">
            <div>
              <h2 className="text-lg font-semibold text-white">Settings</h2>
              <p className="text-sm text-muted mt-1">
                Machine-wide defaults shared across every agent.
              </p>
            </div>
            {onClose ? (
              <button
                onClick={handleClose}
                className="flex shrink-0 items-center gap-2 rounded-lg border border-edge bg-surface px-3 py-2 text-sm text-secondary transition-colors hover:bg-panel hover:text-white"
                aria-label="Close settings"
                title="Close settings"
              >
                <X size={14} />
                <span>Close</span>
              </button>
            ) : null}
          </div>

          {loadError && (
            <div className="rounded-lg border border-red-500/40 bg-red-500/10 px-4 py-3 text-sm text-red-300">
              Failed to load global settings: {loadError}
            </div>
          )}

          <section className="space-y-3">
            <h3 className="text-sm font-semibold text-white">Chat Display</h3>
            <div className="rounded-lg border border-edge bg-background px-4 py-3">
              <div className="flex items-center justify-between gap-4">
                <div>
                  <label className="text-sm font-medium text-white">Show agent thoughts</label>
                  <p className="text-xs text-muted mt-1">
                    Off hides thought chips completely. On shows them as collapsed chips you can
                    expand inline when needed.
                  </p>
                </div>
                <Switch.Root
                  checked={showAgentThoughts}
                  onCheckedChange={(v) => {
                    void setShowAgentThoughts(v);
                  }}
                  className="w-9 h-5 rounded-full bg-edge data-[state=checked]:bg-accent transition-colors outline-none shrink-0"
                >
                  <Switch.Thumb className="block w-4 h-4 rounded-full bg-white shadow translate-x-0.5 data-[state=checked]:translate-x-[18px] transition-transform" />
                </Switch.Root>
              </div>
            </div>
            <div className="rounded-lg border border-edge bg-background px-4 py-3">
              <div className="flex items-center justify-between gap-4">
                <div>
                  <label className="text-sm font-medium text-white">Verbose tool details</label>
                  <p className="text-xs text-muted mt-1">
                    Off shows the shared human-readable tool panels. On also reveals raw input JSON
                    and raw tool result payloads inside expanded tool details.
                  </p>
                </div>
                <Switch.Root
                  checked={showVerboseToolDetails}
                  onCheckedChange={(v) => {
                    void setShowVerboseToolDetails(v);
                  }}
                  className="w-9 h-5 rounded-full bg-edge data-[state=checked]:bg-accent transition-colors outline-none shrink-0"
                >
                  <Switch.Thumb className="block w-4 h-4 rounded-full bg-white shadow translate-x-0.5 data-[state=checked]:translate-x-[18px] transition-transform" />
                </Switch.Root>
              </div>
            </div>
          </section>

          {loaded && <ChannelsSection />}
          {loaded && <AgentDefaultsSection />}
          {loaded && <PermissionRulesSection />}

          <section className="space-y-3">
            <h3 className="text-sm font-semibold text-white">Model Providers</h3>
            <p className="text-xs text-muted">
              API keys for LLM providers. These are shared by all agents that use the same
              provider.
            </p>
            <div className="space-y-2">
              {LLM_PROVIDERS.map((p) => (
                <ProviderKeyRow key={p.value} provider={p.value} label={p.label} />
              ))}
            </div>
          </section>

          <section className="space-y-3">
            <h3 className="text-sm font-semibold text-white">Search Providers</h3>
            <p className="text-xs text-muted">
              API keys for web search. These are shared by all agents with web search enabled.
            </p>
            <div className="space-y-2">
              {SEARCH_PROVIDERS.map((p) => (
                <ProviderKeyRow key={p.value} provider={p.value} label={p.label} />
              ))}
            </div>
          </section>

          <section className="space-y-3">
            <h3 className="text-sm font-semibold text-white">Image Generation</h3>
            <p className="text-xs text-muted">
              Dedicated API key for image generation. v1 uses OpenAI Images with the fixed model
              <span className="font-mono text-secondary"> gpt-image-1</span>.
            </p>
            <div className="space-y-2">
              {IMAGE_GENERATION_PROVIDERS.map((p) => (
                <ProviderKeyRow key={p.value} provider={p.value} label={p.label} />
              ))}
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
