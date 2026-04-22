export function formatWorkItemId(prefix: string | null | undefined, ulid: string): string {
  const safePrefix = (prefix ?? '').trim() || 'ITEM';
  const suffix = ulid.slice(-6).toUpperCase();
  return `${safePrefix}-${suffix}`;
}
