import * as Select from '@radix-ui/react-select';
import { ChevronDown } from 'lucide-react';

import { AgentIdentityConfig } from '../../types';
import { CollapsibleSection } from '../../components/CollapsibleSection';
import {
  AGENT_IDENTITY_PRESETS,
  CUSTOM_IDENTITY_OPTION,
  applyIdentityPreset,
  buildIdentityPromptPreview,
  sanitizeIdentity,
  updateIdentityField,
} from '../../lib/agentIdentity';

interface AgentIdentitySectionProps {
  identity: AgentIdentityConfig;
  onChange: (identity: AgentIdentityConfig) => void;
  agentName: string;
  showPreview?: boolean;
}

export function AgentIdentitySection({
  identity,
  onChange,
  agentName,
  showPreview = false,
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
            <div className="rounded-lg border border-edge bg-background px-3 py-3">
              <p className="text-[11px] uppercase tracking-wide text-secondary">Prompt Preview</p>
              <p className="text-sm text-muted mt-1 leading-relaxed">
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
