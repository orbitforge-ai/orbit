/** Format a duration in milliseconds as a human-readable string. */
export function formatDuration(ms: number | null | undefined): string {
  if (ms == null) return '—';
  if (ms < 1000) return `${ms}ms`;
  const secs = Math.floor(ms / 1000);
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  const remainSecs = secs % 60;
  if (mins < 60) return remainSecs > 0 ? `${mins}m ${remainSecs}s` : `${mins}m`;
  const hours = Math.floor(mins / 60);
  const remainMins = mins % 60;
  return remainMins > 0 ? `${hours}h ${remainMins}m` : `${hours}h`;
}

/** Format elapsed seconds as a live counter string. */
export function formatElapsed(startedAt: string | null): string {
  if (!startedAt) return '0s';
  const elapsed = Date.now() - new Date(startedAt).getTime();
  return formatDuration(elapsed);
}
