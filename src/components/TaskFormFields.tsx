import type { ReactNode } from 'react';

export function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <label className="block text-sm font-medium text-secondary mb-1.5">{label}</label>
      {children}
    </div>
  );
}

export function Row({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex gap-3">
      <dt className="w-28 flex-shrink-0 text-xs text-muted pt-0.5">{label}</dt>
      <dd
        className={`flex-1 text-sm text-white break-all ${mono ? 'font-mono text-green-400' : ''}`}
      >
        {value}
      </dd>
    </div>
  );
}
