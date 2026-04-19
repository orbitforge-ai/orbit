import { X, Shield, Database, Zap, Link, Eye, Workflow } from 'lucide-react';
import { StagedInstall } from '../../api/plugins';

interface Props {
  staged: StagedInstall;
  onConfirm: () => void;
  onCancel: () => void;
  onClose: () => void;
}

export function PluginInstallModal({ staged, onConfirm, onCancel, onClose }: Props) {
  const { manifest } = staged;
  const totalContributions =
    manifest.tools.length +
    manifest.entityTypes.length +
    manifest.oauthProviders.length +
    manifest.workflow.triggers.length +
    manifest.workflow.nodes.length;

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/60">
      <div className="w-full max-w-2xl rounded-lg border border-edge bg-background text-white shadow-xl">
        <header className="flex items-center justify-between border-b border-edge px-5 py-3">
          <div>
            <div className="text-sm uppercase tracking-wide text-muted">Install plugin</div>
            <h2 className="text-lg font-semibold">{manifest.name}</h2>
            <div className="text-xs text-muted">
              {manifest.id} · v{manifest.version}
            </div>
          </div>
          <button className="text-muted hover:text-white" onClick={onClose}>
            <X size={16} />
          </button>
        </header>

        <div className="max-h-[60vh] overflow-auto px-5 py-4 text-sm">
          {manifest.description ? (
            <p className="mb-4 text-secondary">{manifest.description}</p>
          ) : null}

          <ContributionSummary icon={<Zap size={12} />} label="Tools" count={manifest.tools.length}>
            {manifest.tools.map((t) => (
              <li key={t.name}>
                <span className="font-mono text-xs text-white">{t.name}</span>
                {t.description ? (
                  <span className="text-muted"> — {t.description}</span>
                ) : null}
              </li>
            ))}
          </ContributionSummary>

          <ContributionSummary
            icon={<Database size={12} />}
            label="Entity types"
            count={manifest.entityTypes.length}
          >
            {manifest.entityTypes.map((e) => (
              <li key={e.name}>
                <span className="font-mono text-xs text-white">{e.name}</span>
                {e.relations.length ? (
                  <span className="text-muted">
                    {' '}
                    (relates to {e.relations.map((r) => r.to).join(', ')})
                  </span>
                ) : null}
              </li>
            ))}
          </ContributionSummary>

          <ContributionSummary
            icon={<Link size={12} />}
            label="OAuth providers"
            count={manifest.oauthProviders.length}
          >
            {manifest.oauthProviders.map((p) => (
              <li key={p.id}>
                <span className="font-mono text-xs text-white">{p.id}</span>
                <span className="text-muted"> — {p.clientType} client</span>
              </li>
            ))}
          </ContributionSummary>

          <ContributionSummary
            icon={<Workflow size={12} />}
            label="Workflow contributions"
            count={manifest.workflow.triggers.length + manifest.workflow.nodes.length}
          >
            {manifest.workflow.triggers.map((t) => (
              <li key={t.kind}>
                <span className="font-mono text-xs text-white">{t.kind}</span>
                <span className="text-muted"> — trigger: {t.displayName}</span>
              </li>
            ))}
            {manifest.workflow.nodes.map((n) => (
              <li key={n.kind}>
                <span className="font-mono text-xs text-white">{n.kind}</span>
                <span className="text-muted"> — node: {n.displayName}</span>
              </li>
            ))}
          </ContributionSummary>

          <ContributionSummary
            icon={<Eye size={12} />}
            label="Hooks subscribed"
            count={manifest.hooks.subscribe.length}
          >
            {manifest.hooks.subscribe.map((s) => (
              <li key={s}>
                <span className="font-mono text-xs text-white">{s}</span>
              </li>
            ))}
          </ContributionSummary>

          <div className="mt-4 rounded-md border border-edge bg-surface px-3 py-2">
            <div className="mb-1 flex items-center gap-1 text-xs uppercase tracking-wide text-muted">
              <Shield size={11} />
              Permissions (advisory)
            </div>
            <div className="text-xs text-secondary">
              <div>
                <strong>Network:</strong>{' '}
                {manifest.permissions.network.length ? manifest.permissions.network.join(', ') : '—'}
              </div>
              <div>
                <strong>OAuth:</strong>{' '}
                {manifest.permissions.oauth.length ? manifest.permissions.oauth.join(', ') : '—'}
              </div>
              <div>
                <strong>Core entities read:</strong>{' '}
                {manifest.permissions.coreEntities.length
                  ? manifest.permissions.coreEntities.join(', ')
                  : '—'}
              </div>
            </div>
          </div>

          {totalContributions === 0 ? (
            <div className="mt-4 text-xs text-muted">
              This plugin declares no contributions. Installing it is a no-op.
            </div>
          ) : null}
        </div>

        <footer className="flex items-center justify-end gap-2 border-t border-edge px-5 py-3">
          <button
            className="rounded border border-edge px-3 py-1.5 text-sm text-secondary hover:bg-surface"
            onClick={onCancel}
          >
            Cancel
          </button>
          <button
            className="rounded bg-accent px-3 py-1.5 text-sm font-medium text-white hover:bg-accent-hover"
            onClick={onConfirm}
          >
            Install
          </button>
        </footer>
      </div>
    </div>
  );
}

function ContributionSummary({
  icon,
  label,
  count,
  children,
}: {
  icon: React.ReactNode;
  label: string;
  count: number;
  children: React.ReactNode;
}) {
  if (count === 0) return null;
  return (
    <div className="mb-3">
      <div className="mb-1 flex items-center gap-1 text-xs uppercase tracking-wide text-muted">
        {icon}
        {label} ({count})
      </div>
      <ul className="ml-4 list-disc space-y-0.5 text-xs text-secondary">{children}</ul>
    </div>
  );
}
