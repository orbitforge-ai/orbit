/**
 * Build-time feature flags via Vite env variables.
 * Set in .env or .env.local — all must be prefixed with VITE_.
 *
 * Example:
 *   VITE_FEATURE_AVATAR=false   # disable companion avatar entirely
 */

function flag(key: string, fallback = true): boolean {
  const val = (import.meta.env as Record<string, string | undefined>)[key];
  if (val === undefined) return fallback;
  return val !== 'false' && val !== '0';
}

export const FEATURES = {
  /** Animated companion avatar in chat panels. */
  avatar: flag('VITE_FEATURE_AVATAR'),
} as const;
