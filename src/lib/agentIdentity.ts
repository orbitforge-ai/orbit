import { AgentIdentityConfig, AvatarArchetype } from '../types';

export interface AgentIdentityPreset extends AgentIdentityConfig {
  label: string;
}

const DEFAULT_AVATAR_FIELDS = {
  avatarEnabled: false,
  avatarArchetype: 'auto' as AvatarArchetype,
  avatarSpeakAloud: false,
};

export const AGENT_IDENTITY_PRESETS: AgentIdentityPreset[] = [
  {
    presetId: 'balanced_assistant',
    label: 'Balanced Assistant',
    identityName: 'Balanced Assistant',
    voice: 'neutral',
    vibe: 'balanced, clear, and approachable',
    warmth: 55,
    directness: 55,
    humor: 20,
    customNote: '',
    ...DEFAULT_AVATAR_FIELDS,
  },
  {
    presetId: 'warm_guide',
    label: 'Warm Guide',
    identityName: 'Warm Guide',
    voice: 'warm',
    vibe: 'encouraging and supportive',
    warmth: 80,
    directness: 40,
    humor: 25,
    customNote: '',
    ...DEFAULT_AVATAR_FIELDS,
  },
  {
    presetId: 'crisp_operator',
    label: 'Crisp Operator',
    identityName: 'Crisp Operator',
    voice: 'crisp',
    vibe: 'efficient, composed, and no-nonsense',
    warmth: 25,
    directness: 85,
    humor: 5,
    customNote: '',
    ...DEFAULT_AVATAR_FIELDS,
  },
  {
    presetId: 'calm_analyst',
    label: 'Calm Analyst',
    identityName: 'Calm Analyst',
    voice: 'calm',
    vibe: 'measured, thoughtful, and analytical',
    warmth: 40,
    directness: 70,
    humor: 10,
    customNote: '',
    ...DEFAULT_AVATAR_FIELDS,
  },
  {
    presetId: 'playful_creative',
    label: 'Playful Creative',
    identityName: 'Playful Creative',
    voice: 'bright',
    vibe: 'inventive, lively, and imaginative',
    warmth: 70,
    directness: 45,
    humor: 60,
    customNote: '',
    ...DEFAULT_AVATAR_FIELDS,
  },
  {
    presetId: 'steady_coach',
    label: 'Steady Coach',
    identityName: 'Steady Coach',
    voice: 'steady',
    vibe: 'confident, motivating, and grounded',
    warmth: 65,
    directness: 65,
    humor: 15,
    customNote: '',
    ...DEFAULT_AVATAR_FIELDS,
  },
];

export const CUSTOM_IDENTITY_OPTION = {
  presetId: 'custom',
  label: 'Custom',
};

const DEFAULT_PRESET = AGENT_IDENTITY_PRESETS[0];

export function getDefaultAgentIdentity(): AgentIdentityConfig {
  return cloneIdentity(DEFAULT_PRESET);
}

export function resolveIdentityPreset(presetId: string): AgentIdentityPreset | undefined {
  return AGENT_IDENTITY_PRESETS.find((preset) => preset.presetId === presetId);
}

export function cloneIdentity(identity: AgentIdentityConfig): AgentIdentityConfig {
  return {
    ...identity,
    customNote: identity.customNote ?? '',
  };
}

export function applyIdentityPreset(presetId: string): AgentIdentityConfig {
  const preset = resolveIdentityPreset(presetId) ?? DEFAULT_PRESET;
  return cloneIdentity(preset);
}

const VALID_ARCHETYPES = new Set<AvatarArchetype>([
  'auto', 'fox', 'bear', 'owl', 'spark', 'cat', 'bot', 'sage',
]);

export function sanitizeIdentity(identity: AgentIdentityConfig): AgentIdentityConfig {
  const base = identity.presetId === 'custom' ? identity : applyIdentityPreset(identity.presetId);

  // Preserve avatar settings from the incoming identity, not the preset base
  const avatarEnabled = identity.avatarEnabled ?? false;
  const avatarArchetype: AvatarArchetype = VALID_ARCHETYPES.has(identity.avatarArchetype as AvatarArchetype)
    ? (identity.avatarArchetype as AvatarArchetype)
    : 'auto';
  const avatarSpeakAloud = identity.avatarSpeakAloud ?? false;

  return {
    presetId: base.presetId || DEFAULT_PRESET.presetId,
    identityName: clampText(base.identityName, 60) || DEFAULT_PRESET.identityName,
    voice: clampText(base.voice, 40) || DEFAULT_PRESET.voice,
    vibe: clampText(base.vibe, 80) || DEFAULT_PRESET.vibe,
    warmth: clampScore(base.warmth),
    directness: clampScore(base.directness),
    humor: clampScore(base.humor),
    customNote: clampText(base.customNote ?? '', 240),
    avatarEnabled,
    avatarArchetype,
    avatarSpeakAloud,
  };
}

/** Scores identity traits against each archetype and returns the best fit. */
export function selectAvatarArchetype(identity: AgentIdentityConfig): Exclude<AvatarArchetype, 'auto'> {
  if (identity.avatarArchetype !== 'auto') {
    return identity.avatarArchetype as Exclude<AvatarArchetype, 'auto'>;
  }

  // Voice shortcut for named presets
  if (identity.presetId !== 'custom') {
    if (identity.voice === 'warm') return 'bear';
    if (identity.voice === 'crisp') return 'cat';
    if (identity.voice === 'bright') return 'spark';
    if (identity.voice === 'calm') return 'owl';
  }

  const { warmth: w, directness: d, humor: h } = identity;

  const scores: Record<Exclude<AvatarArchetype, 'auto'>, number> = {
    fox:   d * 1.2 + h * 0.8 + w * 0.4,
    bear:  w * 1.5 + h * 0.3 + d * 0.2,
    owl:   d * 1.0 + (100 - w) * 1.0 + (100 - h) * 0.5,
    spark: h * 1.5 + w * 0.8 + (100 - d) * 0.2,
    cat:   d * 1.2 + (100 - w) * 1.2,
    bot:   Math.abs(w - 55) < 15 && Math.abs(d - 55) < 15 ? 500 : 0,
    sage:  (100 - h) * 1.5 + d * 0.3,
  };

  return (Object.entries(scores) as [Exclude<AvatarArchetype, 'auto'>, number][])
    .reduce((best, [arch, score]) => (score > best[1] ? [arch, score] : best), ['bot', 0] as [Exclude<AvatarArchetype, 'auto'>, number])[0];
}

export function updateIdentityField<K extends keyof AgentIdentityConfig>(
  identity: AgentIdentityConfig,
  field: K,
  value: AgentIdentityConfig[K]
): AgentIdentityConfig {
  const next = sanitizeIdentity({
    ...identity,
    [field]: value,
    presetId: identity.presetId === 'custom' ? 'custom' : 'custom',
  });
  return next;
}

export function scoreDescriptor(value: number): 'low' | 'medium' | 'high' {
  if (value <= 33) return 'low';
  if (value <= 66) return 'medium';
  return 'high';
}

export function buildIdentityPromptPreview(
  agentName: string,
  identity: AgentIdentityConfig
): string {
  const resolved = sanitizeIdentity(identity);
  let preview = `You are ${agentName || 'this agent'}. Use the '${resolved.identityName}' identity: ${resolved.vibe}. Speak with a ${resolved.voice} voice style, ${scoreDescriptor(resolved.warmth)} warmth, ${scoreDescriptor(resolved.directness)} directness, and ${scoreDescriptor(resolved.humor)} humor.`;

  if (resolved.customNote) {
    preview += ` Additional identity note: ${resolved.customNote}`;
  }

  return preview;
}

function clampScore(value: number): number {
  const numeric = Number.isFinite(value) ? value : DEFAULT_PRESET.warmth;
  return Math.max(0, Math.min(100, Math.round(numeric)));
}

function clampText(value: string, maxLength: number): string {
  return (value ?? '').trim().slice(0, maxLength);
}
