import { useCallback, useEffect, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { listen } from '@tauri-apps/api/event';
import { X, Link, ScrollText, Database, Info, CheckCircle2, Key } from 'lucide-react';
import { pluginsApi, PluginManifest, PluginOAuthStatus, PluginSecretStatus } from '../../api/plugins';
import { PluginLogo } from './PluginLogo';
import { Input } from '../../components/ui';

type Tab = 'overview' | 'oauth' | 'secrets' | 'entities' | 'logs';

interface Props {
  pluginId: string;
  initialTab?: Tab;
  onClose: () => void;
}

export function PluginDetailDrawer({ pluginId, initialTab, onClose }: Props) {
  const [tab, setTab] = useState<Tab>(initialTab ?? 'overview');
  const manifestQuery = useQuery<PluginManifest | null>({
    queryKey: ['plugin-manifest', pluginId],
    queryFn: () => pluginsApi.getManifest(pluginId),
  });

  return (
    <div className="fixed inset-0 z-30 flex justify-end bg-black/40" onClick={onClose}>
      <div
        className="h-full w-[480px] max-w-full overflow-hidden border-l border-edge bg-background text-white shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="flex items-center justify-between border-b border-edge px-4 py-3">
          <div className="flex items-center gap-3">
            <PluginLogo
              name={manifestQuery.data?.name ?? pluginId}
              src={manifestQuery.data?.iconDataUrl}
              size="sm"
            />
            <div>
              <h2 className="text-sm font-semibold">
                {manifestQuery.data?.name ?? pluginId}
              </h2>
              <div className="text-xs text-muted">{pluginId}</div>
            </div>
          </div>
          <button className="text-muted hover:text-white" onClick={onClose}>
            <X size={16} />
          </button>
        </header>

        <nav className="flex items-center gap-1 border-b border-edge px-3 py-1 text-xs">
          <TabButton icon={<Info size={11} />} label="Overview" active={tab === 'overview'} onClick={() => setTab('overview')} />
          <TabButton icon={<Link size={11} />} label="OAuth" active={tab === 'oauth'} onClick={() => setTab('oauth')} />
          <TabButton icon={<Key size={11} />} label="Secrets" active={tab === 'secrets'} onClick={() => setTab('secrets')} />
          <TabButton icon={<Database size={11} />} label="Entities" active={tab === 'entities'} onClick={() => setTab('entities')} />
          <TabButton icon={<ScrollText size={11} />} label="Live log" active={tab === 'logs'} onClick={() => setTab('logs')} />
        </nav>

        <div className="h-[calc(100%-6rem)] overflow-auto px-4 py-3 text-sm">
          {tab === 'overview' ? <OverviewTab manifest={manifestQuery.data ?? null} /> : null}
          {tab === 'oauth' ? <OAuthTab pluginId={pluginId} manifest={manifestQuery.data ?? null} /> : null}
          {tab === 'secrets' ? <SecretsTab pluginId={pluginId} manifest={manifestQuery.data ?? null} /> : null}
          {tab === 'entities' ? <EntitiesTab pluginId={pluginId} manifest={manifestQuery.data ?? null} /> : null}
          {tab === 'logs' ? <LogsTab pluginId={pluginId} /> : null}
        </div>
      </div>
    </div>
  );
}

function TabButton({
  icon,
  label,
  active,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      className={`flex items-center gap-1 rounded px-2 py-1 ${
        active ? 'bg-surface text-white' : 'text-muted hover:text-secondary'
      }`}
      onClick={onClick}
    >
      {icon}
      {label}
    </button>
  );
}

function OverviewTab({ manifest }: { manifest: PluginManifest | null }) {
  if (!manifest) return <div className="text-muted">Loading…</div>;
  return (
    <div className="space-y-3">
      <div>
        <div className="text-xs uppercase tracking-wide text-muted">Version</div>
        <div>{manifest.version}</div>
      </div>
      <div>
        <div className="text-xs uppercase tracking-wide text-muted">Author</div>
        <div>{manifest.author ?? '—'}</div>
      </div>
      {manifest.description ? (
        <div>
          <div className="text-xs uppercase tracking-wide text-muted">Description</div>
          <div className="text-secondary">{manifest.description}</div>
        </div>
      ) : null}
      <div>
        <div className="text-xs uppercase tracking-wide text-muted">Tools</div>
        <ul className="ml-4 list-disc text-xs text-secondary">
          {manifest.tools.map((t) => (
            <li key={t.name}>
              <span className="font-mono">{t.name}</span>
              {t.description ? ` — ${t.description}` : ''}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

function OAuthTab({
  pluginId,
  manifest,
}: {
  pluginId: string;
  manifest: PluginManifest | null;
}) {
  const queryClient = useQueryClient();
  const [clientId, setClientId] = useState<Record<string, string>>({});
  const [clientSecret, setClientSecret] = useState<Record<string, string>>({});

  const statusQuery = useQuery<PluginOAuthStatus[]>({
    queryKey: ['plugin-oauth-status'],
    queryFn: () => pluginsApi.listOAuthStatus(),
  });
  const providersStatus = statusQuery.data
    ?.find((s) => s.pluginId === pluginId)
    ?.providers ?? [];

  useEffect(() => {
    const unlisten = listen('plugin:oauth:connected', () => {
      queryClient.invalidateQueries({ queryKey: ['plugin-oauth-status'] });
    });
    return () => {
      unlisten.then((u) => u());
    };
  }, [queryClient]);

  const connect = useCallback(
    async (providerId: string, clientType: string) => {
      if (clientType === 'confidential') {
        if (!clientId[providerId]) {
          alert('Paste the OAuth App client_id before connecting.');
          return;
        }
        await pluginsApi.setOAuthConfig(
          pluginId,
          providerId,
          clientId[providerId],
          clientSecret[providerId] || undefined
        );
      }
      await pluginsApi.startOAuth(pluginId, providerId);
    },
    [pluginId, clientId, clientSecret]
  );

  const disconnect = useCallback(
    async (providerId: string) => {
      await pluginsApi.disconnectOAuth(pluginId, providerId);
      queryClient.invalidateQueries({ queryKey: ['plugin-oauth-status'] });
    },
    [pluginId, queryClient]
  );

  if (!manifest) return <div className="text-muted">Loading…</div>;
  if (manifest.oauthProviders.length === 0) {
    return <div className="text-muted">No OAuth providers declared.</div>;
  }

  return (
    <div className="space-y-4">
      {manifest.oauthProviders.map((p) => {
        const connected = providersStatus.find((s) => s.id === p.id)?.connected ?? false;
        return (
          <div key={p.id} className="rounded-lg border border-edge px-3 py-3">
            <div className="flex items-center justify-between">
              <div>
                <div className="flex items-center gap-2">
                  <span className="text-sm font-medium">{p.name}</span>
                  {connected ? (
                    <span className="inline-flex items-center gap-1 rounded bg-success/10 px-1.5 py-0.5 text-[10px] font-medium text-success">
                      <CheckCircle2 size={10} />
                      Connected
                    </span>
                  ) : null}
                </div>
                <div className="text-xs text-muted">
                  {p.clientType} client · scopes: {p.scopes.join(', ') || '—'}
                </div>
              </div>
            </div>
            {!connected && p.clientType === 'confidential' ? (
              <div className="mt-3 space-y-2">
                <Input
                  className="bg-background rounded px-2 py-1 text-xs"
                  placeholder="client_id"
                  value={clientId[p.id] ?? ''}
                  onChange={(e) => setClientId({ ...clientId, [p.id]: e.target.value })}
                />
                <Input
                  className="bg-background rounded px-2 py-1 text-xs"
                  placeholder="client_secret (optional)"
                  type="password"
                  value={clientSecret[p.id] ?? ''}
                  onChange={(e) => setClientSecret({ ...clientSecret, [p.id]: e.target.value })}
                />
              </div>
            ) : null}
            <div className="mt-3 flex items-center justify-end gap-2">
              {connected ? (
                <button
                  className="rounded border border-edge px-2.5 py-1 text-xs text-secondary hover:bg-surface"
                  onClick={() => disconnect(p.id)}
                >
                  Disconnect
                </button>
              ) : (
                <button
                  className="rounded bg-accent px-2.5 py-1 text-xs font-medium text-white hover:bg-accent-hover"
                  onClick={() => connect(p.id, p.clientType)}
                >
                  Connect
                </button>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}

function SecretsTab({
  pluginId,
  manifest,
}: {
  pluginId: string;
  manifest: PluginManifest | null;
}) {
  const queryClient = useQueryClient();
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [saving, setSaving] = useState<string | null>(null);

  const statusQuery = useQuery<PluginSecretStatus[]>({
    queryKey: ['plugin-secret-status'],
    queryFn: () => pluginsApi.listSecretStatus(),
  });
  const secretStatus = statusQuery.data
    ?.find((s) => s.pluginId === pluginId)
    ?.secrets ?? [];

  const save = useCallback(
    async (key: string) => {
      const value = drafts[key]?.trim();
      if (!value) return;
      setSaving(key);
      try {
        await pluginsApi.setSecret(pluginId, key, value);
        setDrafts((d) => ({ ...d, [key]: '' }));
        queryClient.invalidateQueries({ queryKey: ['plugin-secret-status'] });
      } finally {
        setSaving(null);
      }
    },
    [pluginId, drafts, queryClient]
  );

  const clear = useCallback(
    async (key: string) => {
      await pluginsApi.deleteSecret(pluginId, key);
      queryClient.invalidateQueries({ queryKey: ['plugin-secret-status'] });
    },
    [pluginId, queryClient]
  );

  if (!manifest) return <div className="text-muted">Loading…</div>;
  if (manifest.secrets.length === 0) {
    return <div className="text-muted">This plugin declares no secrets.</div>;
  }

  return (
    <div className="space-y-4">
      {manifest.secrets.map((spec) => {
        const hasValue =
          secretStatus.find((s) => s.key === spec.key)?.hasValue ?? false;
        return (
          <div key={spec.key} className="rounded-lg border border-edge px-3 py-3">
            <div className="flex items-center justify-between">
              <div>
                <div className="flex items-center gap-2">
                  <span className="text-sm font-medium">{spec.displayName}</span>
                  {hasValue ? (
                    <span className="inline-flex items-center gap-1 rounded bg-success/10 px-1.5 py-0.5 text-[10px] font-medium text-success">
                      <CheckCircle2 size={10} />
                      Stored
                    </span>
                  ) : null}
                </div>
                <div className="text-xs text-muted">
                  Injected as <span className="font-mono">{spec.envVar}</span>
                </div>
              </div>
            </div>
            {spec.description ? (
              <div className="mt-2 text-xs text-secondary">{spec.description}</div>
            ) : null}
            <div className="mt-3 flex items-center gap-2">
              <Input
                className="flex-1 bg-background rounded px-2 py-1 text-xs"
                placeholder={hasValue ? 'Replace value…' : spec.placeholder ?? 'Paste secret…'}
                type="password"
                autoComplete="off"
                value={drafts[spec.key] ?? ''}
                onChange={(e) => setDrafts({ ...drafts, [spec.key]: e.target.value })}
              />
              <button
                className="rounded bg-accent px-2.5 py-1 text-xs font-medium text-white hover:bg-accent-hover disabled:opacity-50"
                disabled={saving === spec.key || !drafts[spec.key]?.trim()}
                onClick={() => save(spec.key)}
              >
                {saving === spec.key ? 'Saving…' : 'Save'}
              </button>
              {hasValue ? (
                <button
                  className="rounded border border-edge px-2.5 py-1 text-xs text-secondary hover:bg-surface"
                  onClick={() => clear(spec.key)}
                >
                  Clear
                </button>
              ) : null}
            </div>
          </div>
        );
      })}
    </div>
  );
}

function EntitiesTab({
  pluginId,
  manifest,
}: {
  pluginId: string;
  manifest: PluginManifest | null;
}) {
  if (!manifest) return <div className="text-muted">Loading…</div>;
  if (manifest.entityTypes.length === 0) {
    return <div className="text-muted">This plugin declares no entity types.</div>;
  }
  return (
    <div className="space-y-3">
      {manifest.entityTypes.map((e) => (
        <div key={e.name} className="rounded-lg border border-edge px-3 py-3 text-xs">
          <div className="text-sm font-medium text-white">{e.displayName ?? e.name}</div>
          <div className="text-muted">
            <span className="font-mono">{e.name}</span>
            {e.relations.length ? ` · relates to ${e.relations.map((r) => r.to).join(', ')}` : ''}
          </div>
          <button
            className="mt-2 rounded border border-edge px-2 py-1 text-xs text-secondary hover:bg-surface"
            onClick={async () => {
              const rows = await pluginsApi.listEntities(pluginId, e.name, { limit: 50 });
              alert(`${rows.length} ${e.name} rows.`);
            }}
          >
            View rows
          </button>
        </div>
      ))}
    </div>
  );
}

function LogsTab({ pluginId }: { pluginId: string }) {
  const [log, setLog] = useState('');
  useEffect(() => {
    pluginsApi.getRuntimeLog(pluginId, 500).then(setLog);
    const unlisten = listen<string>(`plugin:log:${pluginId}`, (event) => {
      setLog((prev) => (prev ? `${prev}\n${event.payload}` : event.payload));
    });
    return () => {
      unlisten.then((u) => u());
    };
  }, [pluginId]);
  return (
    <pre className="whitespace-pre-wrap rounded border border-edge bg-surface px-3 py-2 text-[11px] leading-4 text-muted">
      {log || '(no output yet — call a tool to spawn the subprocess)'}
    </pre>
  );
}
