import { useCallback, useEffect, useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { listen } from '@tauri-apps/api/event';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { confirm } from '@tauri-apps/plugin-dialog';
import {
  Plug,
  Plus,
  RefreshCw,
  Trash2,
  AlertCircle,
  CheckCircle2,
  Loader2,
  FolderOpen,
} from 'lucide-react';
import {
  pluginsApi,
  PluginManifest,
  PluginOAuthStatus,
  PluginSummary,
  StagedInstall,
} from '../../api/plugins';
import { useSettingsStore } from '../../store/settingsStore';
import { PluginInstallModal } from './PluginInstallModal';
import { PluginDetailDrawer } from './PluginDetailDrawer';
import { PluginLogo } from './PluginLogo';

type DrawerTab = 'overview' | 'oauth' | 'entities' | 'logs';

export function Plugins() {
  const queryClient = useQueryClient();
  const [selectedPluginId, setSelectedPluginId] = useState<string | null>(null);
  const [selectedTab, setSelectedTab] = useState<DrawerTab>('overview');
  const [staged, setStaged] = useState<StagedInstall | null>(null);
  const devMode = useSettingsStore((s) => s.settings.developer.pluginDevMode);

  const plugins = useQuery<PluginSummary[]>({
    queryKey: ['plugins'],
    queryFn: () => pluginsApi.list(),
  });

  const oauthStatus = useQuery<PluginOAuthStatus[]>({
    queryKey: ['plugin-oauth-status'],
    queryFn: () => pluginsApi.listOAuthStatus(),
  });

  const oauthByPlugin = useMemo(() => {
    const map = new Map<string, PluginOAuthStatus>();
    for (const entry of oauthStatus.data ?? []) map.set(entry.pluginId, entry);
    return map;
  }, [oauthStatus.data]);

  useEffect(() => {
    const unlistenChanged = listen('plugins:changed', () => {
      queryClient.invalidateQueries({ queryKey: ['plugins'] });
      queryClient.invalidateQueries({ queryKey: ['plugin-oauth-status'] });
    });
    const unlistenOAuth = listen('plugin:oauth:connected', () => {
      queryClient.invalidateQueries({ queryKey: ['plugin-oauth-status'] });
    });
    return () => {
      unlistenChanged.then((u) => u());
      unlistenOAuth.then((u) => u());
    };
  }, [queryClient]);

  const installFromFile = useCallback(async () => {
    const path = await openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: 'Orbit plugin', extensions: ['zip'] }],
    });
    if (typeof path !== 'string') return;
    try {
      const result = await pluginsApi.stageInstall(path);
      setStaged(result);
    } catch (e) {
      alert(`Install failed: ${e}`);
    }
  }, []);

  const installFromDirectory = useCallback(async () => {
    const path = await openDialog({ multiple: false, directory: true });
    if (typeof path !== 'string') return;
    try {
      const manifest: PluginManifest = await pluginsApi.installFromDirectory(path);
      queryClient.invalidateQueries({ queryKey: ['plugins'] });
      alert(`Installed ${manifest.name} (${manifest.id}) as dev plugin.`);
    } catch (e) {
      alert(`Dev install failed: ${e}`);
    }
  }, [queryClient]);

  const setEnabledMut = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      pluginsApi.setEnabled(id, enabled),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['plugins'] }),
  });

  const reloadMut = useMutation({
    mutationFn: (id: string) => pluginsApi.reload(id),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['plugins'] }),
  });

  const reloadAllMut = useMutation({
    mutationFn: () => pluginsApi.reloadAll(),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['plugins'] }),
  });

  useEffect(() => {
    if (reloadAllMut.status !== 'success' && reloadAllMut.status !== 'error') return;
    const t = setTimeout(() => reloadAllMut.reset(), 2000);
    return () => clearTimeout(t);
  }, [reloadAllMut.status, reloadAllMut.reset]);

  const uninstallMut = useMutation({
    mutationFn: async (id: string) => {
      const ok = await confirm(
        `Uninstall ${id}? Secrets will be removed; entity data is kept. Reinstalling will rehydrate.`,
        { title: 'Uninstall plugin', kind: 'warning' }
      );
      if (!ok) return;
      await pluginsApi.uninstall(id);
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['plugins'] }),
  });

  const rows = useMemo(() => plugins.data ?? [], [plugins.data]);

  return (
    <div className="flex h-full flex-col bg-background text-white">
      <header className="flex items-center justify-between border-b border-edge px-6 py-4">
        <div className="flex items-center gap-2">
          <Plug size={18} className="text-secondary" />
          <h1 className="text-lg font-semibold">Plugins</h1>
          <span className="rounded bg-surface px-2 py-0.5 text-[10px] uppercase tracking-wide text-muted">
            V1
          </span>
        </div>
        <div className="flex items-center gap-2">
          <button
            className="flex items-center gap-1 rounded border border-edge px-3 py-1.5 text-sm text-secondary hover:bg-surface disabled:opacity-70"
            onClick={() => reloadAllMut.mutate()}
            disabled={reloadAllMut.isPending}
            title={
              reloadAllMut.status === 'error'
                ? String(reloadAllMut.error ?? 'Reload failed')
                : undefined
            }
          >
            {reloadAllMut.status === 'pending' ? (
              <Loader2 size={13} className="animate-spin" />
            ) : reloadAllMut.status === 'success' ? (
              <CheckCircle2 size={13} className="text-emerald-400" />
            ) : reloadAllMut.status === 'error' ? (
              <AlertCircle size={13} className="text-red-400" />
            ) : (
              <RefreshCw size={13} />
            )}
            {reloadAllMut.status === 'pending'
              ? 'Reloading…'
              : reloadAllMut.status === 'success'
                ? 'Reloaded'
                : reloadAllMut.status === 'error'
                  ? 'Failed'
                  : 'Reload all'}
          </button>
          {devMode ? (
            <button
              className="flex items-center gap-1 rounded border border-edge px-3 py-1.5 text-sm text-secondary hover:bg-surface"
              onClick={installFromDirectory}
            >
              <FolderOpen size={13} />
              Install from directory
            </button>
          ) : null}
          <button
            className="flex items-center gap-1 rounded bg-accent px-3 py-1.5 text-sm font-medium text-white hover:bg-accent-hover"
            onClick={installFromFile}
          >
            <Plus size={13} />
            Install from file
          </button>
        </div>
      </header>

      <main className="flex-1 overflow-auto px-6 py-4">
        {plugins.isLoading ? (
          <div className="flex items-center gap-2 text-muted">
            <Loader2 size={14} className="animate-spin" />
            Loading…
          </div>
        ) : rows.length === 0 ? (
          <div className="rounded-lg border border-dashed border-edge px-6 py-10 text-center text-muted">
            <Plug size={28} className="mx-auto mb-3 text-muted" />
            <p className="mb-2 text-sm">No plugins installed.</p>
            <p className="text-xs">
              Install a `.zip` plugin or point at a local directory in developer mode.
            </p>
          </div>
        ) : (
          <div className="grid gap-3 grid-cols-1 sm:grid-cols-2 lg:grid-cols-3">
            {rows.map((plugin) => (
              <PluginCard
                key={plugin.id}
                plugin={plugin}
                oauth={oauthByPlugin.get(plugin.id) ?? null}
                onOpen={() => {
                  setSelectedTab('overview');
                  setSelectedPluginId(plugin.id);
                }}
                onToggle={(enabled) =>
                  setEnabledMut.mutate({ id: plugin.id, enabled })
                }
                onReload={() => reloadMut.mutate(plugin.id)}
                onUninstall={() => uninstallMut.mutate(plugin.id)}
                onConnectOAuth={async () => {
                  const status = oauthByPlugin.get(plugin.id);
                  const target = status?.providers.find(
                    (p) => !p.connected && (p.clientType !== 'confidential' || p.hasClientId),
                  );
                  if (target) {
                    try {
                      await pluginsApi.startOAuth(plugin.id, target.id);
                      return;
                    } catch (e) {
                      alert(`Connect failed: ${e}`);
                      return;
                    }
                  }
                  setSelectedTab('oauth');
                  setSelectedPluginId(plugin.id);
                }}
              />
            ))}
          </div>
        )}
      </main>

      {staged ? (
        <PluginInstallModal
          staged={staged}
          onClose={() => setStaged(null)}
          onConfirm={async () => {
            try {
              await pluginsApi.confirmInstall(staged.stagingId);
              queryClient.invalidateQueries({ queryKey: ['plugins'] });
            } finally {
              setStaged(null);
            }
          }}
          onCancel={async () => {
            try {
              await pluginsApi.cancelInstall(staged.stagingId);
            } finally {
              setStaged(null);
            }
          }}
        />
      ) : null}

      {selectedPluginId ? (
        <PluginDetailDrawer
          pluginId={selectedPluginId}
          initialTab={selectedTab}
          onClose={() => setSelectedPluginId(null)}
        />
      ) : null}
    </div>
  );
}

interface PluginCardProps {
  plugin: PluginSummary;
  oauth: PluginOAuthStatus | null;
  onOpen: () => void;
  onToggle: (enabled: boolean) => void;
  onReload: () => void;
  onUninstall: () => void;
  onConnectOAuth: () => void;
}

function PluginCard({
  plugin,
  oauth,
  onOpen,
  onToggle,
  onReload,
  onUninstall,
  onConnectOAuth,
}: PluginCardProps) {
  const needsOAuth = oauth?.anyNeedsConnect ?? false;
  return (
    <div className="rounded-lg border border-edge bg-background px-4 py-3">
      <div className="flex items-start justify-between gap-3">
        <button
          className="flex flex-1 items-start gap-3 text-left"
          onClick={onOpen}
          aria-label={`Open plugin ${plugin.name}`}
        >
          <PluginLogo name={plugin.name} src={plugin.iconDataUrl} size="sm" />
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-sm font-medium text-white">{plugin.name}</span>
              {plugin.bundled ? (
                <span className="rounded bg-surface px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-muted">
                  Bundled
                </span>
              ) : null}
              {plugin.dev ? (
                <span className="rounded bg-surface px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-muted">
                  Dev
                </span>
              ) : null}
            </div>
            <div className="mt-0.5 text-xs text-muted">{plugin.id}</div>
            <div className="mt-1 text-xs text-secondary">v{plugin.version}</div>
            {plugin.description ? (
              <div className="mt-2 line-clamp-2 text-xs text-secondary">
                {plugin.description}
              </div>
            ) : null}
          </div>
        </button>
        <StatusDot running={plugin.running} enabled={plugin.enabled} />
      </div>
      {needsOAuth && plugin.enabled ? (
        <div className="mt-3 flex items-center justify-between rounded border border-warning/40 bg-warning/10 px-2 py-2 text-xs">
          <div className="flex items-center gap-1 text-warning">
            <AlertCircle size={12} />
            <span>
              {oauth?.providers.length === 1
                ? `${oauth.providers[0]?.name} not connected`
                : `${oauth?.providers.filter((p) => !p.connected).length ?? 0} providers not connected`}
            </span>
          </div>
          <button
            className="rounded bg-accent px-2.5 py-1 text-[11px] font-medium text-white hover:bg-accent-hover"
            onClick={(e) => {
              e.stopPropagation();
              onConnectOAuth();
            }}
          >
            Connect
          </button>
        </div>
      ) : null}
      <div className="mt-3 flex items-center justify-between border-t border-edge pt-3">
        <label className="flex items-center gap-2 text-xs text-secondary">
          <input
            type="checkbox"
            checked={plugin.enabled}
            onChange={(e) => onToggle(e.target.checked)}
          />
          Enabled
        </label>
        <div className="flex items-center gap-1">
          <button
            className="rounded p-1 text-muted hover:bg-surface hover:text-secondary"
            onClick={onReload}
            title="Reload"
          >
            <RefreshCw size={12} />
          </button>
          <button
            className="rounded p-1 text-muted hover:bg-surface hover:text-danger"
            onClick={onUninstall}
            title="Uninstall"
          >
            <Trash2 size={12} />
          </button>
        </div>
      </div>
    </div>
  );
}

function StatusDot({ running, enabled }: { running: boolean; enabled: boolean }) {
  if (!enabled) {
    return (
      <span title="Disabled" className="inline-flex items-center">
        <AlertCircle size={14} className="text-muted" />
      </span>
    );
  }
  if (running) {
    return (
      <span title="Running" className="inline-flex items-center">
        <CheckCircle2 size={14} className="text-success" />
      </span>
    );
  }
  return (
    <span title="Idle" className="inline-flex items-center">
      <CheckCircle2 size={14} className="text-secondary" />
    </span>
  );
}
