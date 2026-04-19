import { useEffect, useState } from 'react';
import { Check, AlertTriangle } from 'lucide-react';
import { chatApi } from '../../api/chat';
import { onAgentConfigChanged } from '../../events/agentEvents';
import { onChatContextUpdate, onCompactionStatus } from '../../events/runEvents';

interface ContextGaugeProps {
  sessionId: string;
  agentId?: string;
  onCompacted?: () => void;
}

function getColor(percent: number): string {
  if (percent >= 80) return 'var(--color-failure)';
  if (percent >= 65) return 'var(--color-orange)';
  if (percent >= 50) return 'var(--color-yellow)';
  return 'var(--color-success)';
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}k`;
  return String(n);
}

export function ContextGauge({ sessionId, agentId, onCompacted }: ContextGaugeProps) {
  const [inputTokens, setInputTokens] = useState(0);
  const [contextWindow, setContextWindow] = useState(0);
  const [usagePercent, setUsagePercent] = useState(0);
  const [compacting, setCompacting] = useState(false);
  const [justCompacted, setJustCompacted] = useState(false);
  const [compactionFailed, setCompactionFailed] = useState(false);

  async function refreshContextUsage() {
    const usage = await chatApi.getContextUsage(sessionId);
    setInputTokens(usage.inputTokens);
    setContextWindow(usage.contextWindowSize);
    setUsagePercent(usage.usagePercent);
  }

  // Load initial context usage on mount
  useEffect(() => {
    refreshContextUsage().catch(() => {});
  }, [sessionId]);

  // Subscribe to real-time context updates
  useEffect(() => {
    const unsub = onChatContextUpdate((payload) => {
      if (payload.sessionId !== sessionId) return;
      setInputTokens(payload.inputTokens);
      setContextWindow(payload.contextWindowSize);
      setUsagePercent(payload.usagePercent);
    });

    return () => {
      unsub.then((fn) => fn()).catch(() => {});
    };
  }, [sessionId]);

  // Refresh when the active agent's model config changes
  useEffect(() => {
    if (!agentId) return;

    const unsub = onAgentConfigChanged((payload) => {
      if (payload.agentId !== agentId) return;
      refreshContextUsage().catch(() => {});
    });

    return () => {
      unsub.then((fn) => fn()).catch(() => {});
    };
  }, [agentId, sessionId]);

  // Subscribe to background compaction status events
  useEffect(() => {
    const unsub = onCompactionStatus((payload) => {
      if (payload.sessionId !== sessionId) return;
      if (payload.status === 'started') {
        setCompacting(true);
        setCompactionFailed(false);
      } else if (payload.status === 'completed') {
        setCompacting(false);
        setJustCompacted(true);
        onCompacted?.();
        setTimeout(() => setJustCompacted(false), 2000);
      } else if (payload.status === 'failed') {
        setCompacting(false);
        setCompactionFailed(true);
        setTimeout(() => setCompactionFailed(false), 4000);
      }
    });

    return () => {
      unsub.then((fn) => fn()).catch(() => {});
    };
  }, [sessionId, onCompacted]);

  async function handleCompact() {
    if (compacting) return;
    setCompacting(true);
    setJustCompacted(false);
    setCompactionFailed(false);
    try {
      await chatApi.compactSession(sessionId);
      await refreshContextUsage();
      // Show success state
      setJustCompacted(true);
      onCompacted?.();
      setTimeout(() => setJustCompacted(false), 2000);
    } catch (err) {
      console.error('Compaction failed:', err);
      setCompactionFailed(true);
      setTimeout(() => setCompactionFailed(false), 4000);
    }
    setCompacting(false);
  }

  // Don't render until we have data
  if (contextWindow === 0) return null;

  const size = 20;
  const strokeWidth = 2;
  const radius = (size - strokeWidth) / 2;
  const circumference = 2 * Math.PI * radius;
  const fillPercent = Math.min(usagePercent, 100);
  const dashOffset = circumference - (fillPercent / 100) * circumference;
  const color = justCompacted
    ? 'var(--color-success)'
    : compactionFailed
      ? 'var(--color-failure)'
      : getColor(usagePercent);

  const tooltip = compacting
    ? 'Compacting...'
    : justCompacted
      ? 'Compaction complete'
      : compactionFailed
        ? 'Compaction failed — click to retry'
        : usagePercent >= 65
          ? `${formatTokens(inputTokens)} / ${formatTokens(contextWindow)} tokens (${usagePercent.toFixed(1)}%) — auto-compaction may be in progress, or click to compact manually`
          : `${formatTokens(inputTokens)} / ${formatTokens(contextWindow)} tokens (${usagePercent.toFixed(1)}%) — click to compact`;

  return (
    <button
      onClick={handleCompact}
      disabled={compacting}
      className="relative inline-flex items-center justify-center cursor-pointer hover:opacity-80 disabled:opacity-40 transition-opacity"
      title={tooltip}
    >
      <svg
        width={size}
        height={size}
        className={`transform -rotate-90 ${compacting ? 'animate-spin' : ''}`}
      >
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          fill="none"
          stroke="var(--color-surface)"
          strokeWidth={strokeWidth}
        />
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          fill="none"
          stroke={color}
          strokeWidth={strokeWidth}
          strokeDasharray={circumference}
          strokeDashoffset={dashOffset}
          strokeLinecap="round"
          className="transition-all duration-700 ease-out"
          style={{ opacity: 0.8 }}
        />
      </svg>
      {compacting ? null : justCompacted ? (
        <Check size={10} className="absolute text-emerald-400" strokeWidth={3} />
      ) : compactionFailed ? (
        <AlertTriangle size={9} className="absolute" style={{ color: 'var(--color-failure)' }} />
      ) : (
        <span
          className="absolute text-[7px] font-mono tabular-nums leading-none"
          style={{ color, opacity: 0.9 }}
        >
          {Math.round(usagePercent)}
        </span>
      )}
    </button>
  );
}
