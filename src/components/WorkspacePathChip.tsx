import { useState } from 'react';
import { Check, Copy } from 'lucide-react';
import { cn } from '../lib/cn';

interface WorkspacePathChipProps {
  path: string;
  className?: string;
}

export function WorkspacePathChip({ path, className }: WorkspacePathChipProps) {
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    try {
      await navigator.clipboard.writeText(path);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch (err) {
      console.warn('Failed to copy workspace path:', err);
    }
  }

  return (
    <button
      type="button"
      onClick={handleCopy}
      title={copied ? 'Copied' : path}
      aria-label="Copy workspace path"
      className={cn(
        'flex items-center gap-1.5 min-w-0 rounded-full border border-edge bg-surface px-2.5 py-1 text-[11px] font-mono text-muted hover:text-white hover:border-edge-hover transition-colors',
        className
      )}
    >
      {copied ? (
        <Check size={11} className="text-emerald-400 shrink-0" />
      ) : (
        <Copy size={11} className="shrink-0" />
      )}
      <span className="truncate">{path}</span>
    </button>
  );
}
