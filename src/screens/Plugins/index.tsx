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
  PluginSummary,
  StagedInstall,
} from '../../api/plugins';
import { PluginInstallModal } from './PluginInstallModal';
import { PluginDetailDrawer } from './PluginDetailDrawer';

export function Plugins() {
  const queryClient = useQueryClient();
  const [selectedPluginId, setSelectedPluginId] = useState<string | null>(null);
  const [staged, setStaged] = useState<StagedInstall | null>(null);
  const [devMode, setDevMode] = useState(false);

  const plugins = useQuery<PluginSummary[]>({
    queryKey: ['plugins'],
    queryFn: () => pluginsApi.list(),
  });

  useEffect(() => {
    const unlisten = listen('plugins:changed', () => {
      queryClient.invalidateQueries({ queryKey: ['plugins'] });
    });
    return () => {
      unlisten.then((u) => u());
    };
  }, [queryClient]);

  useEffect(() => {
    // Read dev mode flag. Best-effort: the global settings API would be
    // cleaner, but for now we call a lightweight invoke if available.
    import('@tauri-apps/api/core').then(({ invoke }) => {
      invoke<{ developer?: { pluginDevMode?: boolean } }>('get_global_settings')
        .then((s) => setDevMode(Boolean(s.developer?.pluginDevMode)))
        .catch(() => {});
    });
  }, []);

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
            className="flex items-center gap-1 rounded border border-edge px-3 py-1.5 text-sm text-secondary hover:bg-surface"
            onClick={() => reloadAllMut.mutate()}
            disabled={reloadAllMut.isPending}
          >
            <RefreshCw size={13} />
            Reload all
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
            className="flex items-center gap-1 rounded bg-primary px-3 py-1.5 text-sm text-white hover:bg-primary-hover"
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
                onOpen={() => setSelectedPluginId(plugin.id)}
                onToggle={(enabled) =>
                  setEnabledMut.mutate({ id: plugin.id, enabled })
                }
                onReload={() => reloadMut.mutate(plugin.id)}
                onUninstall={() => uninstallMut.mutate(plugin.id)}
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
          onClose={() => setSelectedPluginId(null)}
        />
      ) : null}
    </div>
  );
}

interface PluginCardProps {
  plugin: PluginSummary;
  onOpen: () => void;
  onToggle: (enabled: boolean) => void;
  onReload: () => void;
  onUninstall: () => void;
}

function PluginCard({ plugin, onOpen, onToggle, onReload, onUninstall }: PluginCardProps) {
  return (
    <div className="rounded-lg border border-edge bg-background px-4 py-3">
      <div className="flex items-start justify-between gap-3">
        <button
          className="flex-1 text-left"
          onClick={onOpen}
          aria-label={`Open plugin ${plugin.name}`}
        >
          <div className="flex items-center gap-2">
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
        </button>
        <StatusDot running={plugin.running} enabled={plugin.enabled} />
      </div>
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
