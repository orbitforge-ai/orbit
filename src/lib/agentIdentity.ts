import { AgentIdentityConfig } from '../types';

export interface AgentIdentityPreset extends AgentIdentityConfig {
  label: string;
}

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

export function sanitizeIdentity(identity: AgentIdentityConfig): AgentIdentityConfig {
  const base = identity.presetId === 'custom' ? identity : applyIdentityPreset(identity.presetId);

  return {
    presetId: base.presetId || DEFAULT_PRESET.presetId,
    identityName: clampText(base.identityName, 60) || DEFAULT_PRESET.identityName,
    voice: clampText(base.voice, 40) || DEFAULT_PRESET.voice,
    vibe: clampText(base.vibe, 80) || DEFAULT_PRESET.vibe,
    warmth: clampScore(base.warmth),
    directness: clampScore(base.directness),
    humor: clampScore(base.humor),
    customNote: clampText(base.customNote ?? '', 240),
  };
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
