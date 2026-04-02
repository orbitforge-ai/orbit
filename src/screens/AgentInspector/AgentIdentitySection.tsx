import * as Select from '@radix-ui/react-select';
import * as Switch from '@radix-ui/react-switch';
import { ChevronDown } from 'lucide-react';

import { AgentIdentityConfig, AvatarArchetype } from '../../types';
import { CollapsibleSection } from '../../components/CollapsibleSection';
import {
  AGENT_IDENTITY_PRESETS,
  CUSTOM_IDENTITY_OPTION,
  applyIdentityPreset,
  buildIdentityPromptPreview,
  sanitizeIdentity,
  selectAvatarArchetype,
  updateIdentityField,
} from '../../lib/agentIdentity';
import { AVATAR_SVG_MAP, ARCHETYPE_LABELS } from '../../components/avatar/avatarSvgs';

interface AgentIdentitySectionProps {
  identity: AgentIdentityConfig;
  onChange: (identity: AgentIdentityConfig) => void;
  agentName: string;
  showPreview?: boolean;
  roleInstructions?: string;
}

export function AgentIdentitySection({
  identity,
  onChange,
  agentName,
  showPreview = false,
  roleInstructions,
}: AgentIdentitySectionProps) {
  const resolved = sanitizeIdentity(identity);
  const isCustom = resolved.presetId === 'custom';

  function handlePresetChange(presetId: string) {
    if (presetId === 'custom') {
      onChange({ ...resolved, presetId: 'custom' });
      return;
    }
    onChange(applyIdentityPreset(presetId));
  }

  function handleFieldChange<K extends keyof AgentIdentityConfig>(
    field: K,
    value: AgentIdentityConfig[K]
  ) {
    onChange(updateIdentityField(resolved, field, value));
  }

  const selectedPreset =
    AGENT_IDENTITY_PRESETS.find((preset) => preset.presetId === resolved.presetId)?.label ??
    CUSTOM_IDENTITY_OPTION.label;

  return (
    <section className="space-y-4">
      <div>
        <h4 className="text-sm font-semibold text-white">Identity</h4>
        <p className="text-xs text-muted mt-1">
          Give this agent a distinct personality profile that shapes how it speaks and shows up in
          the system prompt.
        </p>
      </div>

      <div>
        <label className="text-xs text-muted mb-1 block">Preset</label>
        <Select.Root value={resolved.presetId} onValueChange={handlePresetChange}>
          <Select.Trigger className="flex items-center justify-between w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent">
            <Select.Value />
            <Select.Icon>
              <ChevronDown size={14} className="text-muted" />
            </Select.Icon>
          </Select.Trigger>
          <Select.Portal>
            <Select.Content className="rounded-lg bg-surface border border-edge shadow-xl overflow-hidden z-50">
              <Select.Viewport className="p-1">
                {AGENT_IDENTITY_PRESETS.map((preset) => (
                  <Select.Item
                    key={preset.presetId}
                    value={preset.presetId}
                    className="px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-accent/20"
                  >
                    <Select.ItemText>{preset.label}</Select.ItemText>
                  </Select.Item>
                ))}
                <Select.Item
                  value={CUSTOM_IDENTITY_OPTION.presetId}
                  className="px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-accent/20"
                >
                  <Select.ItemText>{CUSTOM_IDENTITY_OPTION.label}</Select.ItemText>
                </Select.Item>
              </Select.Viewport>
            </Select.Content>
          </Select.Portal>
        </Select.Root>
      </div>

      <AvatarSection identity={resolved} onChange={onChange} agentName={agentName} />

      <CollapsibleSection
        title="Advanced Identity Options"
        description="Tune voice, vibe, and personality traits when you want something more custom."
        badge={
          <span className="rounded-md border border-edge bg-background px-2 py-1 text-[11px] text-muted">
            {selectedPreset}
          </span>
        }
      >
        <div className="space-y-4">
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="text-xs text-muted mb-1 block">Identity Name</label>
              <input
                type="text"
                maxLength={60}
                value={resolved.identityName}
                onChange={(e) => handleFieldChange('identityName', e.target.value)}
                className="w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent"
              />
            </div>
            <div>
              <label className="text-xs text-muted mb-1 block">Voice</label>
              <input
                type="text"
                maxLength={40}
                value={resolved.voice}
                onChange={(e) => handleFieldChange('voice', e.target.value)}
                className="w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent"
              />
            </div>
          </div>

          <div>
            <label className="text-xs text-muted mb-1 block">Vibe</label>
            <input
              type="text"
              maxLength={80}
              value={resolved.vibe}
              onChange={(e) => handleFieldChange('vibe', e.target.value)}
              className="w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent"
            />
            <span className="text-[10px] text-muted mt-0.5 block">
              Short personality summary used in the system prompt.
            </span>
          </div>

          <div className="grid grid-cols-3 gap-3">
            <TraitInput
              label="Warmth"
              value={resolved.warmth}
              onChange={(value) => handleFieldChange('warmth', value)}
            />
            <TraitInput
              label="Directness"
              value={resolved.directness}
              onChange={(value) => handleFieldChange('directness', value)}
            />
            <TraitInput
              label="Humor"
              value={resolved.humor}
              onChange={(value) => handleFieldChange('humor', value)}
            />
          </div>

          <div>
            <label className="text-xs text-muted mb-1 block">Custom Note</label>
            <textarea
              rows={3}
              maxLength={240}
              value={resolved.customNote ?? ''}
              onChange={(e) => handleFieldChange('customNote', e.target.value)}
              placeholder="Optional extra note for how this identity should come through."
              className="w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent resize-none leading-relaxed"
            />
            <span className="text-[10px] text-muted mt-0.5 block">Optional and kept concise.</span>
          </div>

          {showPreview && (
            <div className="rounded-lg border border-edge bg-background px-3 py-3 space-y-2">
              <p className="text-[11px] uppercase tracking-wide text-secondary">Prompt Preview</p>
              {roleInstructions && roleInstructions.trim() && (
                <p className="text-sm text-muted leading-relaxed border-b border-edge pb-2">
                  {roleInstructions.trim()}
                </p>
              )}
              <p className="text-sm text-muted leading-relaxed">
                {buildIdentityPromptPreview(agentName || 'this agent', resolved)}
              </p>
            </div>
          )}

          {!isCustom && (
            <p className="text-[11px] text-muted">
              Editing any field turns this preset into a custom identity while keeping the current
              values.
            </p>
          )}
        </div>
      </CollapsibleSection>
    </section>
  );
}

const ARCHETYPE_OPTIONS: { value: AvatarArchetype; label: string }[] = [
  { value: 'auto', label: 'Auto (from traits)' },
  { value: 'fox',   label: 'Fox' },
  { value: 'bear',  label: 'Bear' },
  { value: 'owl',   label: 'Owl' },
  { value: 'spark', label: 'Spark' },
  { value: 'cat',   label: 'Cat' },
  { value: 'bot',   label: 'Bot' },
  { value: 'sage',  label: 'Sage' },
];

function AvatarSection({
  identity,
  onChange,
  agentName,
}: {
  identity: AgentIdentityConfig;
  onChange: (identity: AgentIdentityConfig) => void;
  agentName: string;
}) {
  const resolvedArchetype = selectAvatarArchetype(identity);
  const PreviewSvg = AVATAR_SVG_MAP[resolvedArchetype];

  return (
    <CollapsibleSection
      title="Companion Avatar"
      description="Enable an animated avatar character that reacts to agent activity in the chat panel."
      badge={
        identity.avatarEnabled ? (
          <span className="rounded-md border border-accent/40 bg-accent/10 px-2 py-1 text-[11px] text-accent-hover">
            {identity.avatarArchetype === 'auto'
              ? `Auto — ${ARCHETYPE_LABELS[resolvedArchetype]}`
              : ARCHETYPE_LABELS[resolvedArchetype]}
          </span>
        ) : (
          <span className="rounded-md border border-edge bg-background px-2 py-1 text-[11px] text-muted">
            Off
          </span>
        )
      }
    >
      <div className="space-y-4">
        {/* Enable toggle */}
        <div className="flex items-center justify-between">
          <div>
            <label className="text-xs text-white">Enable Avatar</label>
            <p className="text-[10px] text-muted mt-0.5">
              Show an animated character in the chat panel.
            </p>
          </div>
          <Switch.Root
            checked={identity.avatarEnabled}
            onCheckedChange={(checked) =>
              onChange({ ...identity, avatarEnabled: checked })
            }
            className="w-9 h-5 rounded-full relative bg-edge data-[state=checked]:bg-accent transition-colors outline-none"
          >
            <Switch.Thumb className="block w-4 h-4 rounded-full bg-white shadow-sm transition-transform translate-x-0.5 data-[state=checked]:translate-x-4" />
          </Switch.Root>
        </div>

        {identity.avatarEnabled && (
          <>
            {/* Archetype picker + preview */}
            <div className="flex items-start gap-4">
              <div className="flex-1">
                <label className="text-xs text-muted mb-1 block">Character</label>
                <Select.Root
                  value={identity.avatarArchetype}
                  onValueChange={(val) =>
                    onChange({ ...identity, avatarArchetype: val as AvatarArchetype })
                  }
                >
                  <Select.Trigger className="flex items-center justify-between w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent">
                    <Select.Value />
                    <Select.Icon>
                      <ChevronDown size={14} className="text-muted" />
                    </Select.Icon>
                  </Select.Trigger>
                  <Select.Portal>
                    <Select.Content className="rounded-lg bg-surface border border-edge shadow-xl overflow-hidden z-50">
                      <Select.Viewport className="p-1">
                        {ARCHETYPE_OPTIONS.map((opt) => (
                          <Select.Item
                            key={opt.value}
                            value={opt.value}
                            className="px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-accent/20"
                          >
                            <Select.ItemText>{opt.label}</Select.ItemText>
                          </Select.Item>
                        ))}
                      </Select.Viewport>
                    </Select.Content>
                  </Select.Portal>
                </Select.Root>
              </div>

              {/* Live preview */}
              <div className="flex flex-col items-center gap-1 pt-4">
                <div className="w-14 h-14 avatar-idle">
                  <PreviewSvg size={56} />
                </div>
                <span className="text-[10px] text-muted">
                  {agentName || 'Preview'}
                </span>
              </div>
            </div>

            {/* Speak aloud toggle */}
            <div className="flex items-center justify-between">
              <div>
                <label className="text-xs text-white">Speak Aloud</label>
                <p className="text-[10px] text-muted mt-0.5">
                  Read agent responses using text-to-speech.
                </p>
              </div>
              <Switch.Root
                checked={identity.avatarSpeakAloud}
                onCheckedChange={(checked) =>
                  onChange({ ...identity, avatarSpeakAloud: checked })
                }
                className="w-9 h-5 rounded-full relative bg-edge data-[state=checked]:bg-accent transition-colors outline-none"
              >
                <Switch.Thumb className="block w-4 h-4 rounded-full bg-white shadow-sm transition-transform translate-x-0.5 data-[state=checked]:translate-x-4" />
              </Switch.Root>
            </div>
          </>
        )}
      </div>
    </CollapsibleSection>
  );
}

function TraitInput({
  label,
  value,
  onChange,
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
}) {
  return (
    <div>
      <label className="text-xs text-muted mb-1 block">{label}</label>
      <input
        type="number"
        min={0}
        max={100}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent"
      />
      <span className="text-[10px] text-muted mt-0.5 block">0 - 100</span>
    </div>
  );
}
