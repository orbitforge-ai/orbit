import { useEffect, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import * as Switch from '@radix-ui/react-switch';
import { Hash, Plus, Radio, RefreshCw, Trash2, X } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { ChannelBinding } from '../../types';

interface ListenChannelsTabProps {
  agentId: string;
}

interface PluginSummary {
  id: string;
  name: string;
}

/**
 * A Discord `list_channels` response is opaque on the Rust side (passthrough of
 * whatever the plugin returned). Today Orbit's Discord plugin returns an array
 * of guilds with nested channels; other providers may differ. The shape below
 * is a best-effort interpretation — any field may be missing for non-Discord
 * plugins and the UI falls back to a flat list.
 */
interface RawChannel {
  id: string;
  name?: string;
  type?: string;
  parentId?: string | null;
  guildId?: string;
  guildName?: string;
}

interface Guild {
  id: string;
  name: string;
  channels: RawChannel[];
}

export function ListenChannelsTab({ agentId }: ListenChannelsTabProps) {
  const queryClient = useQueryClient();
  const [adding, setAdding] = useState(false);

  const { data: plugins = [] } = useQuery<PluginSummary[]>({
    queryKey: ['trigger-capable-plugins'],
    queryFn: () => invoke('list_trigger_capable_plugins'),
  });

  const { data: bindings = [], isLoading } = useQuery<ChannelBinding[]>({
    queryKey: ['listen-bindings', agentId],
    queryFn: () => invoke('list_agent_listen_bindings', { agentId }),
  });

  async function persist(next: ChannelBinding[]) {
    await invoke('set_agent_listen_bindings', { agentId, bindings: next });
    queryClient.invalidateQueries({ queryKey: ['listen-bindings', agentId] });
  }

  async function handleAdd(binding: ChannelBinding) {
    await persist([...bindings, binding]);
    setAdding(false);
  }

  async function handleDelete(id: string) {
    await persist(bindings.filter((b) => b.id !== id));
  }

  async function handleToggle(id: string, field: 'autoRespond' | 'mentionOnly', value: boolean) {
    await persist(bindings.map((b) => (b.id === id ? { ...b, [field]: value } : b)));
  }

  return (
    <div className="p-6 space-y-6 h-full overflow-y-auto">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Radio size={16} className="text-accent-hover" />
          <h4 className="text-sm font-semibold text-white">Listen Channels</h4>
        </div>
        {plugins.length > 0 && !adding && (
          <button
            onClick={() => setAdding(true)}
            className="flex items-center gap-1 px-2.5 py-1.5 rounded-lg bg-accent hover:bg-accent-hover text-white text-xs font-medium transition-colors"
          >
            <Plus size={12} /> Bind channel
          </button>
        )}
      </div>

      <p className="text-xs text-muted leading-relaxed">
        Bind this agent to a plugin channel or thread. When someone posts there, the agent runs and
        replies in place. Requires an enabled plugin that declares a listener trigger (for example
        Discord).
      </p>

      {plugins.length === 0 && (
        <div className="rounded-lg border border-edge bg-background p-4 text-xs text-muted">
          No trigger-capable plugins are enabled. Install and enable a plugin like{' '}
          <span className="font-mono text-secondary">com.orbit.discord</span> to bind channels.
        </div>
      )}

      {adding && (
        <BindingForm
          plugins={plugins}
          onCancel={() => setAdding(false)}
          onSubmit={handleAdd}
          existing={bindings}
        />
      )}

      <div className="space-y-2">
        {isLoading ? (
          <div className="text-xs text-muted">Loading bindings…</div>
        ) : bindings.length === 0 ? (
          <div className="rounded-lg border border-dashed border-edge bg-background/50 p-6 text-center text-xs text-muted">
            No channels bound yet.
          </div>
        ) : (
          bindings.map((binding) => {
            const plugin = plugins.find((p) => p.id === binding.pluginId);
            return (
              <div
                key={binding.id}
                className="flex items-center justify-between gap-3 rounded-lg border border-edge bg-background px-4 py-3"
              >
                <div className="flex min-w-0 items-center gap-3">
                  <Hash size={14} className="text-muted" />
                  <div className="min-w-0">
                    <div className="truncate text-sm text-white">
                      {binding.label ?? binding.providerChannelId}
                      {binding.providerThreadId && (
                        <span className="ml-1 text-muted">
                          › thread {binding.providerThreadId.slice(-6)}
                        </span>
                      )}
                    </div>
                    <div className="mt-0.5 text-[11px] text-muted">
                      {plugin?.name ?? binding.pluginId} · id {binding.providerChannelId}
                    </div>
                  </div>
                </div>
                <div className="flex items-center gap-4">
                  <ToggleRow
                    label="Auto-respond"
                    checked={binding.autoRespond}
                    onChange={(v) => handleToggle(binding.id, 'autoRespond', v)}
                  />
                  <ToggleRow
                    label="Mention only"
                    checked={binding.mentionOnly ?? false}
                    onChange={(v) => handleToggle(binding.id, 'mentionOnly', v)}
                  />
                  <button
                    onClick={() => handleDelete(binding.id)}
                    className="rounded-lg p-1.5 text-muted transition-colors hover:text-danger"
                    title="Remove binding"
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}

function ToggleRow({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <label className="flex items-center gap-2 text-[11px] text-muted">
      {label}
      <Switch.Root
        checked={checked}
        onCheckedChange={onChange}
        className="relative h-4 w-7 rounded-full bg-edge data-[state=checked]:bg-accent transition-colors"
      >
        <Switch.Thumb className="block h-3 w-3 translate-x-0.5 rounded-full bg-white transition-transform data-[state=checked]:translate-x-3.5" />
      </Switch.Root>
    </label>
  );
}

function BindingForm({
  plugins,
  existing,
  onCancel,
  onSubmit,
}: {
  plugins: PluginSummary[];
  existing: ChannelBinding[];
  onCancel: () => void;
  onSubmit: (binding: ChannelBinding) => void | Promise<void>;
}) {
  const [pluginId, setPluginId] = useState<string>(plugins[0]?.id ?? '');
  const [guildId, setGuildId] = useState<string | null>(null);
  const [channelId, setChannelId] = useState<string>('');
  const [channelLabel, setChannelLabel] = useState<string>('');
  const [threadId, setThreadId] = useState<string>('');
  const [autoRespond, setAutoRespond] = useState(true);
  const [mentionOnly, setMentionOnly] = useState(false);
  const [manualMode, setManualMode] = useState(false);

  const {
    data: channelsRaw,
    isLoading,
    isError,
    error,
    refetch,
    isFetching,
  } = useQuery<unknown, Error>({
    queryKey: ['plugin-channels', pluginId, guildId],
    queryFn: () =>
      invoke('plugin_list_channels', {
        pluginId,
        guildId: guildId ?? null,
      }),
    enabled: Boolean(pluginId) && !manualMode,
    retry: false,
  });

  const guilds = parseGuilds(channelsRaw);

  // When a guild list returns, auto-pick the first one if nothing selected.
  useEffect(() => {
    if (!guildId && guilds.length === 1) {
      setGuildId(guilds[0].id);
    }
  }, [guilds, guildId]);

  const currentChannels = guildId
    ? guilds.find((g) => g.id === guildId)?.channels ?? []
    : guilds.flatMap((g) => g.channels);

  function handleSubmit() {
    if (!pluginId || !channelId.trim()) return;
    const id = cryptoRandomId();
    const duplicate = existing.some(
      (b) =>
        b.pluginId === pluginId &&
        b.providerChannelId === channelId.trim() &&
        (b.providerThreadId ?? null) === (threadId.trim() || null),
    );
    if (duplicate) return;
    onSubmit({
      id,
      pluginId,
      providerChannelId: channelId.trim(),
      providerThreadId: threadId.trim() || undefined,
      label: channelLabel.trim() || undefined,
      autoRespond,
      mentionOnly: mentionOnly || undefined,
    });
  }

  return (
    <div className="rounded-lg border border-edge bg-background p-4 space-y-3">
      <div className="flex items-center justify-between">
        <h5 className="text-xs font-semibold text-white">New channel binding</h5>
        <button
          onClick={onCancel}
          className="rounded-lg p-1 text-muted hover:text-white"
          title="Cancel"
        >
          <X size={13} />
        </button>
      </div>

      <div className="grid grid-cols-2 gap-3">
        <label className="space-y-1">
          <span className="text-[11px] text-muted">Provider</span>
          <select
            value={pluginId}
            onChange={(e) => {
              setPluginId(e.target.value);
              setGuildId(null);
              setChannelId('');
              setChannelLabel('');
            }}
            className="w-full rounded-lg border border-edge bg-surface px-2 py-1.5 text-sm text-white focus:outline-none focus:border-accent"
          >
            {plugins.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        </label>

        {guilds.length > 1 && (
          <label className="space-y-1">
            <span className="text-[11px] text-muted">Workspace / Guild</span>
            <select
              value={guildId ?? ''}
              onChange={(e) => {
                setGuildId(e.target.value || null);
                setChannelId('');
                setChannelLabel('');
              }}
              className="w-full rounded-lg border border-edge bg-surface px-2 py-1.5 text-sm text-white focus:outline-none focus:border-accent"
            >
              <option value="">— any —</option>
              {guilds.map((g) => (
                <option key={g.id} value={g.id}>
                  {g.name}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>

      <div className="flex items-center justify-between">
        <span className="text-[11px] text-muted">
          {manualMode ? 'Manual entry' : 'Channel from plugin'}
        </span>
        <div className="flex items-center gap-2">
          {!manualMode && (
            <button
              onClick={() => refetch()}
              disabled={isFetching}
              className="flex items-center gap-1 rounded-lg px-2 py-1 text-[11px] text-muted hover:text-white disabled:opacity-50"
              title="Reload channels from plugin"
            >
              <RefreshCw size={11} className={isFetching ? 'animate-spin' : ''} /> Refresh
            </button>
          )}
          <button
            onClick={() => setManualMode((m) => !m)}
            className="rounded-lg px-2 py-1 text-[11px] text-muted hover:text-white"
          >
            {manualMode ? 'Use list' : 'Enter id manually'}
          </button>
        </div>
      </div>

      {manualMode ? (
        <div className="grid grid-cols-2 gap-3">
          <label className="space-y-1">
            <span className="text-[11px] text-muted">Channel ID</span>
            <input
              type="text"
              value={channelId}
              onChange={(e) => setChannelId(e.target.value)}
              placeholder="e.g. 1234567890"
              className="w-full rounded-lg border border-edge bg-surface px-2 py-1.5 text-sm text-white focus:outline-none focus:border-accent"
            />
          </label>
          <label className="space-y-1">
            <span className="text-[11px] text-muted">Label (optional)</span>
            <input
              type="text"
              value={channelLabel}
              onChange={(e) => setChannelLabel(e.target.value)}
              placeholder="#general"
              className="w-full rounded-lg border border-edge bg-surface px-2 py-1.5 text-sm text-white focus:outline-none focus:border-accent"
            />
          </label>
        </div>
      ) : isLoading ? (
        <div className="text-xs text-muted">Loading channels…</div>
      ) : isError ? (
        <div className="space-y-1">
          <div className="text-xs text-danger">Plugin returned an error. Enter id manually or inspect logs.</div>
          <pre className="max-h-40 overflow-auto whitespace-pre-wrap rounded border border-edge bg-surface p-2 text-[11px] font-mono text-muted">
            {formatError(error)}
          </pre>
          <PluginRuntimeLog pluginId={pluginId} />
        </div>
      ) : currentChannels.length === 0 ? (
        <div className="text-xs text-muted">No channels returned by plugin.</div>
      ) : (
        <label className="space-y-1 block">
          <span className="text-[11px] text-muted">Channel</span>
          <select
            value={channelId}
            onChange={(e) => {
              const picked = currentChannels.find((c) => c.id === e.target.value);
              setChannelId(e.target.value);
              setChannelLabel(picked?.name ? `#${picked.name}` : '');
            }}
            className="w-full rounded-lg border border-edge bg-surface px-2 py-1.5 text-sm text-white focus:outline-none focus:border-accent"
          >
            <option value="">— pick a channel —</option>
            {currentChannels.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name ? `#${c.name}` : c.id}
                {c.type ? ` (${c.type})` : ''}
              </option>
            ))}
          </select>
        </label>
      )}

      <label className="space-y-1 block">
        <span className="text-[11px] text-muted">
          Thread ID (optional — leave empty to listen to the whole channel)
        </span>
        <input
          type="text"
          value={threadId}
          onChange={(e) => setThreadId(e.target.value)}
          placeholder="thread snowflake / ts"
          className="w-full rounded-lg border border-edge bg-surface px-2 py-1.5 text-sm text-white focus:outline-none focus:border-accent"
        />
      </label>

      <div className="flex items-center gap-6 pt-1">
        <ToggleRow label="Auto-respond" checked={autoRespond} onChange={setAutoRespond} />
        <ToggleRow label="Mention only" checked={mentionOnly} onChange={setMentionOnly} />
      </div>

      <div className="flex justify-end gap-2 pt-1">
        <button
          onClick={onCancel}
          className="rounded-lg px-3 py-1.5 text-xs text-muted hover:text-white"
        >
          Cancel
        </button>
        <button
          onClick={handleSubmit}
          disabled={!pluginId || !channelId.trim()}
          className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white hover:bg-accent-hover disabled:opacity-50"
        >
          Bind
        </button>
      </div>
    </div>
  );
}

function PluginRuntimeLog({ pluginId }: { pluginId: string }) {
  const { data: log = '', refetch } = useQuery<string>({
    queryKey: ['plugin-runtime-log', pluginId],
    queryFn: () => invoke('get_plugin_runtime_log', { pluginId, tailLines: 200 }),
    refetchInterval: 2000,
  });
  return (
    <details className="text-[11px] text-muted">
      <summary className="cursor-pointer hover:text-white">Plugin stderr (click to expand)</summary>
      <div className="mt-1 flex items-center gap-2">
        <button
          onClick={() => refetch()}
          className="rounded px-2 py-0.5 text-[10px] text-muted hover:text-white"
        >
          Refresh
        </button>
      </div>
      <pre className="mt-1 max-h-60 overflow-auto whitespace-pre-wrap rounded border border-edge bg-surface p-2 font-mono">
        {log || '(no output yet)'}
      </pre>
    </details>
  );
}

function formatError(err: unknown): string {
  if (!err) return 'Unknown error';
  if (typeof err === 'string') return err;
  if (err instanceof Error) return err.message;
  try {
    return JSON.stringify(err, null, 2);
  } catch {
    return String(err);
  }
}

function cryptoRandomId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return `b_${Math.random().toString(36).slice(2)}${Date.now().toString(36)}`;
}

/**
 * Parse the plugin's `list_channels` response into a uniform shape. Best-effort;
 * Discord returns `[{id,name,channels:[...]}]` and other plugins may vary.
 */
function parseGuilds(raw: unknown): Guild[] {
  if (!raw) return [];
  // Array of guild-like objects.
  if (Array.isArray(raw)) {
    const first = raw[0];
    if (first && typeof first === 'object' && Array.isArray((first as any).channels)) {
      return (raw as any[]).map((g) => ({
        id: String(g.id ?? ''),
        name: String(g.name ?? g.id ?? 'workspace'),
        channels: toChannels(g.channels, g.id, g.name),
      }));
    }
    // Flat channel array.
    return [{ id: '', name: '', channels: toChannels(raw, undefined, undefined) }];
  }
  if (typeof raw === 'object') {
    const obj = raw as any;
    if (Array.isArray(obj.guilds)) {
      return (obj.guilds as any[]).map((g) => ({
        id: String(g.id ?? ''),
        name: String(g.name ?? g.id ?? 'workspace'),
        channels: toChannels(g.channels, g.id, g.name),
      }));
    }
    if (Array.isArray(obj.channels)) {
      // `{ channels: [...] }` comes back from a guild-scoped call. Borrow the
      // guild id from the first channel so upstream `guilds.find(g => g.id === guildId)`
      // matches the currently-selected guild instead of mis-matching on `''`.
      const first = (obj.channels as any[])[0] ?? {};
      const gid = String(first.guild_id ?? first.guildId ?? '');
      return [{ id: gid, name: '', channels: toChannels(obj.channels, gid || undefined, undefined) }];
    }
  }
  return [];
}

function toChannels(list: unknown, guildId: unknown, guildName: unknown): RawChannel[] {
  if (!Array.isArray(list)) return [];
  return list
    .map((c) => {
      if (!c || typeof c !== 'object') return null;
      const obj = c as any;
      return {
        id: String(obj.id ?? ''),
        name: obj.name ? String(obj.name) : undefined,
        type: obj.type ? String(obj.type) : undefined,
        parentId: obj.parentId ?? obj.parent_id ?? null,
        guildId: guildId !== undefined ? String(guildId) : undefined,
        guildName: guildName !== undefined ? String(guildName) : undefined,
      } as RawChannel;
    })
    .filter((c): c is RawChannel => !!c && !!c.id);
}
