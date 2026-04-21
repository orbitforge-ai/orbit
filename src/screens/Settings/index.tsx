import { useEffect, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import * as Switch from '@radix-ui/react-switch';
import {
  AlertTriangle,
  Check,
  Key,
  Plus,
  Terminal,
  Trash2,
  X,
} from 'lucide-react';
import { Input, Select, SelectContent, SelectItem, SelectTrigger, SelectValue, SimpleSelect } from '../../components/ui';
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
import { invoke } from '@tauri-apps/api/core';
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
          <Input
            type="password"
            placeholder={`Enter ${label} API key...`}
            value={keyInput}
            onChange={(e) => setKeyInput(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handleSave()}
            autoFocus
            className="flex-1 px-3 py-2 font-mono"
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
      mode: 'webhook',
    });
  }

  async function handleSave() {
    if (!draft) return;
    if (!draft.name.trim()) return;
    const isBot = draft.mode === 'bot';
    if (isBot) {
      if (!draft.pluginId || !draft.providerChannelId?.trim()) return;
    } else {
      if (!draft.webhookUrl.trim()) return;
    }
    try {
      const normalized: ChannelConfig = {
        ...draft,
        name: draft.name.trim(),
        webhookUrl: isBot ? '' : draft.webhookUrl.trim(),
        providerChannelId: isBot ? draft.providerChannelId?.trim() : undefined,
        providerThreadId: isBot ? draft.providerThreadId?.trim() || undefined : undefined,
        pluginId: isBot ? draft.pluginId : undefined,
      };
      await upsertChannel(normalized);
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
                  {channel.mode === 'bot' && (
                    <span className="rounded bg-accent/20 px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-accent-hover">
                      bot
                    </span>
                  )}
                </div>
                <p className="mt-1 truncate font-mono text-xs text-muted">
                  {channel.mode === 'bot'
                    ? `${channel.pluginId ?? '?'} · ${channel.providerChannelId ?? '?'}${
                        channel.providerThreadId ? ` › ${channel.providerThreadId}` : ''
                      }`
                    : channel.webhookUrl}
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
        <ChannelDraftForm
          draft={draft}
          setDraft={setDraft}
          onCancel={() => setDraft(null)}
          onSave={handleSave}
        />
      )}
    </section>
  );
}

interface PluginSummary {
  id: string;
  name: string;
}

function ChannelDraftForm({
  draft,
  setDraft,
  onCancel,
  onSave,
}: {
  draft: ChannelConfig;
  setDraft: (draft: ChannelConfig) => void;
  onCancel: () => void;
  onSave: () => void;
}) {
  const isBot = draft.mode === 'bot';

  const { data: plugins = [] } = useQuery<PluginSummary[]>({
    queryKey: ['trigger-capable-plugins'],
    queryFn: () => invoke('list_trigger_capable_plugins'),
    enabled: isBot,
  });

  // When switching to bot mode, preselect the first available plugin.
  useEffect(() => {
    if (isBot && !draft.pluginId && plugins.length > 0) {
      setDraft({ ...draft, pluginId: plugins[0].id });
    }
  }, [isBot, draft, plugins, setDraft]);

  const { data: channelsRaw } = useQuery<unknown>({
    queryKey: ['plugin-channels', draft.pluginId],
    queryFn: () =>
      invoke('plugin_list_channels', { pluginId: draft.pluginId, guildId: null }),
    enabled: isBot && Boolean(draft.pluginId),
  });

  const flatChannels = flattenChannels(channelsRaw);

  const saveDisabled =
    !draft.name.trim() ||
    (isBot ? !draft.pluginId || !draft.providerChannelId?.trim() : !draft.webhookUrl.trim());

  return (
    <div className="space-y-3 rounded-lg border border-accent/60 bg-background px-4 py-3">
      <div className="flex items-center gap-2">
        <span className="text-xs text-muted">Mode</span>
        <div className="flex rounded-md border border-edge bg-surface p-0.5">
          <ModeButton
            active={!isBot}
            onClick={() =>
              setDraft({
                ...draft,
                mode: 'webhook',
                pluginId: undefined,
                providerChannelId: undefined,
                providerThreadId: undefined,
              })
            }
          >
            Webhook
          </ModeButton>
          <ModeButton
            active={isBot}
            onClick={() =>
              setDraft({ ...draft, mode: 'bot', webhookUrl: '', type: 'discord' })
            }
          >
            Bot (plugin)
          </ModeButton>
        </div>
      </div>

      <div className="grid grid-cols-2 gap-3">
        <label className="flex flex-col gap-1">
          <span className="text-xs text-muted">Name</span>
          <Input
            value={draft.name}
            onChange={(e) => setDraft({ ...draft, name: e.target.value })}
            placeholder="e.g. #ops-alerts"
            className="rounded-md px-2.5 py-1.5"
          />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-xs text-muted">Type</span>
          <SimpleSelect
            value={draft.type}
            onValueChange={(v) => setDraft({ ...draft, type: v as ChannelType })}
            className="rounded-md px-2.5 py-1.5"
            options={CHANNEL_TYPE_OPTIONS.map((o) => ({ value: o.value, label: o.label }))}
          />
        </label>
      </div>

      {isBot ? (
        <div className="space-y-3">
          <label className="flex flex-col gap-1">
            <span className="text-xs text-muted">Plugin</span>
            {plugins.length === 0 ? (
              <div className="rounded-md border border-edge bg-surface px-2.5 py-1.5 text-xs text-muted">
                No trigger-capable plugins are enabled. Install one (e.g.{' '}
                <span className="font-mono">com.orbit.discord</span>) and enable it first.
              </div>
            ) : (
              <SimpleSelect
                value={draft.pluginId ?? ''}
                onValueChange={(v) =>
                  setDraft({
                    ...draft,
                    pluginId: v,
                    providerChannelId: undefined,
                  })
                }
                className="rounded-md px-2.5 py-1.5"
                options={plugins.map((p) => ({ value: p.id, label: p.name }))}
              />
            )}
          </label>

          <div className="grid grid-cols-2 gap-3">
            <label className="flex flex-col gap-1">
              <span className="text-xs text-muted">Channel</span>
              {flatChannels.length > 0 ? (
                <SimpleSelect
                  value={draft.providerChannelId ?? ''}
                  onValueChange={(v) => {
                    const picked = flatChannels.find((c) => c.id === v);
                    setDraft({
                      ...draft,
                      providerChannelId: v,
                      name: draft.name || (picked?.name ? `#${picked.name}` : draft.name),
                    });
                  }}
                  placeholder="— pick a channel —"
                  className="rounded-md px-2.5 py-1.5"
                  options={flatChannels.map((c) => ({
                    value: c.id,
                    label: c.name ? `#${c.name}` : c.id,
                  }))}
                />
              ) : (
                <Input
                  value={draft.providerChannelId ?? ''}
                  onChange={(e) =>
                    setDraft({ ...draft, providerChannelId: e.target.value })
                  }
                  placeholder="channel id / snowflake"
                  className="rounded-md px-2.5 py-1.5 font-mono"
                />
              )}
            </label>
            <label className="flex flex-col gap-1">
              <span className="text-xs text-muted">Thread (optional)</span>
              <Input
                value={draft.providerThreadId ?? ''}
                onChange={(e) => setDraft({ ...draft, providerThreadId: e.target.value })}
                placeholder="thread snowflake / ts"
                className="rounded-md px-2.5 py-1.5 font-mono"
              />
            </label>
          </div>
          <p className="text-[11px] text-muted">
            Outbound messages go through the plugin's <span className="font-mono">send_message</span>{' '}
            tool. The plugin must be installed, enabled, and authenticated.
          </p>
        </div>
      ) : (
        <label className="flex flex-col gap-1">
          <span className="text-xs text-muted">Webhook URL</span>
          <Input
            value={draft.webhookUrl}
            onChange={(e) => setDraft({ ...draft, webhookUrl: e.target.value })}
            placeholder="https://hooks.slack.com/services/..."
            className="rounded-md px-2.5 py-1.5 font-mono"
          />
          <span className="text-[11px] text-muted">
            {CHANNEL_TYPE_OPTIONS.find((o) => o.value === draft.type)?.hint}
          </span>
        </label>
      )}

      <div className="flex justify-end gap-2">
        <button
          onClick={onCancel}
          className="rounded-md px-3 py-1.5 text-xs text-muted hover:text-white transition-colors"
        >
          Cancel
        </button>
        <button
          onClick={onSave}
          disabled={saveDisabled}
          className="rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-accent-hover disabled:opacity-50"
        >
          Save channel
        </button>
      </div>
    </div>
  );
}

function ModeButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      className={`rounded px-2.5 py-1 text-xs font-medium transition-colors ${
        active ? 'bg-accent text-white' : 'text-muted hover:text-white'
      }`}
    >
      {children}
    </button>
  );
}

/**
 * Flatten the plugin's `list_channels` response into a flat channel array. The
 * payload shape is plugin-specific; Discord returns guild-wrapped channels but
 * other providers may return a flat list. Best-effort.
 */
function flattenChannels(
  raw: unknown,
): { id: string; name?: string }[] {
  if (!raw) return [];
  if (Array.isArray(raw)) {
    const first = raw[0];
    if (first && typeof first === 'object' && Array.isArray((first as any).channels)) {
      return (raw as any[]).flatMap((g) =>
        Array.isArray(g.channels)
          ? g.channels
              .filter((c: any) => c && c.id)
              .map((c: any) => ({ id: String(c.id), name: c.name ? String(c.name) : undefined }))
          : [],
      );
    }
    return (raw as any[])
      .filter((c) => c && c.id)
      .map((c) => ({ id: String(c.id), name: c.name ? String(c.name) : undefined }));
  }
  if (typeof raw === 'object') {
    const obj = raw as any;
    if (Array.isArray(obj.channels)) return flattenChannels(obj.channels);
    if (Array.isArray(obj.guilds)) return flattenChannels(obj.guilds);
  }
  return [];
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
          <Select
            value={agentDefaults.permissionMode}
            onValueChange={(v) =>
              updateAgentDefaults({ permissionMode: v as AgentDefaults['permissionMode'] })
            }
          >
            <SelectTrigger className="rounded-md px-2.5 py-1.5 min-w-[130px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {PERMISSION_MODE_OPTIONS.map((opt) => (
                <SelectItem key={opt.value} value={opt.value}>
                  <div>
                    <div>{opt.label}</div>
                    <div className="text-[11px] text-muted">{opt.description}</div>
                  </div>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
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
          <SimpleSelect
            value={agentDefaults.webSearchProvider}
            onValueChange={(v) => updateAgentDefaults({ webSearchProvider: v })}
            className="rounded-md px-2.5 py-1.5 min-w-[130px]"
            options={SEARCH_PROVIDERS.map((o) => ({ value: o.value, label: o.label }))}
          />
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

// ─── Developer ───────────────────────────────────────────────────────────────

function DeveloperSection() {
  const pluginDevMode = useSettingsStore((s) => s.settings.developer.pluginDevMode);
  const updateDeveloper = useSettingsStore((s) => s.updateDeveloper);

  return (
    <section className="space-y-3">
      <h3 className="text-sm font-semibold text-white">Developer</h3>
      <div className="rounded-lg border border-edge bg-background px-4 py-3">
        <div className="flex items-center justify-between gap-4">
          <div>
            <label className="text-sm font-medium text-white">Plugin dev mode</label>
            <p className="text-xs text-muted mt-1">
              Unlocks &ldquo;Install from directory&rdquo; on the Plugins screen, shows MCP
              wire logs, and skips manifest hash checks. Plugins run unsandboxed from disk.
            </p>
          </div>
          <Switch.Root
            checked={pluginDevMode}
            onCheckedChange={(v) => {
              void updateDeveloper({ pluginDevMode: v });
            }}
            className="w-9 h-5 rounded-full bg-edge data-[state=checked]:bg-accent transition-colors outline-none shrink-0"
            aria-label="Plugin dev mode"
          >
            <Switch.Thumb className="block w-4 h-4 rounded-full bg-white shadow translate-x-0.5 data-[state=checked]:translate-x-[18px] transition-transform" />
          </Switch.Root>
        </div>
      </div>
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
          {loaded && <DeveloperSection />}

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
