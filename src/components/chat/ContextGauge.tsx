import { useEffect, useState } from 'react';
import { Check } from 'lucide-react';
import { chatApi } from '../../api/chat';
import { onChatContextUpdate } from '../../events/runEvents';

interface ContextGaugeProps {
  sessionId: string;
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

export function ContextGauge({ sessionId, onCompacted }: ContextGaugeProps) {
  const [inputTokens, setInputTokens] = useState(0);
  const [contextWindow, setContextWindow] = useState(0);
  const [usagePercent, setUsagePercent] = useState(0);
  const [compacting, setCompacting] = useState(false);
  const [justCompacted, setJustCompacted] = useState(false);

  // Load initial context usage on mount
  useEffect(() => {
    chatApi
      .getContextUsage(sessionId)
      .then((usage) => {
        setInputTokens(usage.inputTokens);
        setContextWindow(usage.contextWindowSize);
        setUsagePercent(usage.usagePercent);
      })
      .catch(() => {});
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
      unsub.then((fn) => fn());
    };
  }, [sessionId]);

  async function handleCompact() {
    if (compacting) return;
    setCompacting(true);
    setJustCompacted(false);
    try {
      await chatApi.compactSession(sessionId);
      // Refetch updated usage
      const usage = await chatApi.getContextUsage(sessionId);
      setInputTokens(usage.inputTokens);
      setContextWindow(usage.contextWindowSize);
      setUsagePercent(usage.usagePercent);
      // Show success state
      setJustCompacted(true);
      onCompacted?.();
      setTimeout(() => setJustCompacted(false), 2000);
    } catch (err) {
      console.error('Compaction failed:', err);
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
  const color = justCompacted ? 'var(--color-success)' : getColor(usagePercent);

  return (
    <button
      onClick={handleCompact}
      disabled={compacting}
      className="relative inline-flex items-center justify-center cursor-pointer hover:opacity-80 disabled:opacity-40 transition-opacity"
      title={
        compacting
          ? 'Compacting...'
          : justCompacted
            ? 'Compaction complete'
            : `${formatTokens(inputTokens)} / ${formatTokens(contextWindow)} tokens (${usagePercent.toFixed(1)}%) — click to compact`
      }
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
