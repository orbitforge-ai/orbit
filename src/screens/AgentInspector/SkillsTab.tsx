import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Plus,
  Trash2,
  ChevronDown,
  ChevronRight,
  Package,
  Globe,
  User,
  Cpu,
  Eye,
  EyeOff,
} from 'lucide-react';
import { confirm } from '../../lib/dialog';

import { skillsApi } from '../../api/skills';
import { workspaceApi } from '../../api/workspace';
import { Input, Textarea } from '../../components/ui';
import { SkillInfo, SkillSource } from '../../types';

interface SkillsTabProps {
  agentId: string;
}

const SOURCE_LABELS: Record<SkillSource, { label: string; icon: typeof Package }> = {
  agent_local: { label: 'Agent', icon: User },
  orbit_global: { label: 'Global', icon: Globe },
  standard: { label: 'Standard', icon: Package },
  built_in: { label: 'Built-in', icon: Cpu },
};

export function SkillsTab({ agentId }: SkillsTabProps) {
  const queryClient = useQueryClient();
  const [showCreate, setShowCreate] = useState(false);
  const [expandedSkill, setExpandedSkill] = useState<string | null>(null);
  const [skillContent, setSkillContent] = useState<Record<string, string>>({});

  const { data: skills = [] } = useQuery({
    queryKey: ['skills', agentId],
    queryFn: () => skillsApi.list(agentId),
  });

  async function handleToggleSkill(skill: SkillInfo) {
    const config = await workspaceApi.getConfig(agentId);
    let disabled = [...(config.disabledSkills || [])];

    if (skill.enabled) {
      disabled.push(skill.name);
    } else {
      disabled = disabled.filter((n) => n !== skill.name);
    }

    await workspaceApi.updateConfig(agentId, { ...config, disabledSkills: disabled });
    queryClient.invalidateQueries({ queryKey: ['skills', agentId] });
  }

  async function handleExpand(skillName: string) {
    if (expandedSkill === skillName) {
      setExpandedSkill(null);
      return;
    }
    setExpandedSkill(skillName);
    if (!skillContent[skillName]) {
      try {
        const content = await skillsApi.getContent(agentId, skillName);
        setSkillContent((prev) => ({ ...prev, [skillName]: content }));
      } catch (err) {
        setSkillContent((prev) => ({ ...prev, [skillName]: `Error: ${err}` }));
      }
    }
  }

  async function handleDelete(skillName: string) {
    if (!(await confirm(`Delete skill "${skillName}"? This cannot be undone.`))) return;
    try {
      await skillsApi.delete(agentId, skillName);
      queryClient.invalidateQueries({ queryKey: ['skills', agentId] });
      if (expandedSkill === skillName) setExpandedSkill(null);
    } catch (err) {
      console.error('Failed to delete skill:', err);
    }
  }

  return (
    <div className="p-6 space-y-6 h-full overflow-y-auto">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h4 className="text-sm font-semibold text-white">Agent Skills</h4>
          <p className="text-xs text-muted mt-1">
            Skills extend agent capabilities with specialized instructions loaded on demand.
          </p>
        </div>
        <button
          onClick={() => setShowCreate(true)}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover text-white text-xs font-medium transition-colors"
        >
          <Plus size={12} /> Add Skill
        </button>
      </div>

      {/* Create form */}
      {showCreate && (
        <CreateSkillForm
          agentId={agentId}
          onCreated={() => {
            setShowCreate(false);
            queryClient.invalidateQueries({ queryKey: ['skills', agentId] });
          }}
          onCancel={() => setShowCreate(false)}
        />
      )}

      {/* Skill list */}
      {skills.length === 0 ? (
        <div className="text-sm text-muted py-8 text-center">
          No skills discovered. Add a skill or place one in{' '}
          <code className="text-xs bg-surface px-1 py-0.5 rounded">~/.orbit/skills/</code>
        </div>
      ) : (
        <div className="space-y-2">
          {skills.map((skill) => {
            const sourceInfo = SOURCE_LABELS[skill.source];
            const SourceIcon = sourceInfo.icon;
            const isExpanded = expandedSkill === skill.name;

            return (
              <div
                key={skill.name}
                className={`rounded-xl border transition-colors ${
                  skill.enabled ? 'border-edge bg-surface' : 'border-edge bg-surface/50 opacity-60'
                }`}
              >
                {/* Skill header */}
                <div
                  className="flex items-center gap-3 px-4 py-3 cursor-pointer"
                  onClick={() => handleExpand(skill.name)}
                >
                  {isExpanded ? (
                    <ChevronDown size={14} className="text-muted shrink-0" />
                  ) : (
                    <ChevronRight size={14} className="text-muted shrink-0" />
                  )}

                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="text-sm font-medium text-white">{skill.name}</span>
                      <span className="flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium bg-accent/10 text-accent-light">
                        <SourceIcon size={10} />
                        {sourceInfo.label}
                      </span>
                      {skill.active && (
                        <span className="px-1.5 py-0.5 rounded text-[10px] font-medium bg-warning/10 text-warning">
                          Active in Session
                        </span>
                      )}
                    </div>
                    <p className="text-xs text-muted mt-0.5 truncate">{skill.description}</p>
                  </div>

                  <div className="flex items-center gap-1.5 shrink-0">
                    {/* Toggle enable/disable */}
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        handleToggleSkill(skill);
                      }}
                      className={`p-1.5 rounded-lg transition-colors ${
                        skill.enabled
                          ? 'text-emerald-400 hover:bg-emerald-500/10'
                          : 'text-muted hover:bg-surface'
                      }`}
                      title={skill.enabled ? 'Disable skill' : 'Enable skill'}
                    >
                      {skill.enabled ? <Eye size={14} /> : <EyeOff size={14} />}
                    </button>

                    {/* Delete (agent-local only) */}
                    {skill.source === 'agent_local' && (
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          handleDelete(skill.name);
                        }}
                        className="p-1.5 rounded-lg text-red-400 hover:bg-red-500/10 transition-colors"
                        title="Delete skill"
                      >
                        <Trash2 size={14} />
                      </button>
                    )}
                  </div>
                </div>

                {/* Expanded content */}
                {isExpanded && (
                  <div className="border-t border-edge px-4 py-3">
                    {skillContent[skill.name] ? (
                      <pre className="text-xs text-secondary whitespace-pre-wrap font-mono leading-relaxed max-h-80 overflow-y-auto">
                        {skillContent[skill.name]}
                      </pre>
                    ) : (
                      <p className="text-xs text-muted">Loading...</p>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

// ─── Create Skill Form ─────────────────────────────────────────────────────

function CreateSkillForm({
  agentId,
  onCreated,
  onCancel,
}: {
  agentId: string;
  onCreated: () => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [body, setBody] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleCreate() {
    if (!name.trim() || !description.trim()) return;
    setSaving(true);
    setError(null);
    try {
      await skillsApi.create(agentId, name.trim(), description.trim(), body.trim());
      onCreated();
    } catch (err) {
      setError(String(err));
    }
    setSaving(false);
  }

  return (
    <div className="rounded-xl border border-accent/30 bg-accent/5 p-4 space-y-3">
      <h5 className="text-sm font-semibold text-white">New Skill</h5>

      <div>
        <label className="text-xs text-muted mb-1 block">Name</label>
        <Input
          placeholder="my-skill (lowercase, hyphens only)"
          value={name}
          onChange={(e) => setName(e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, ''))}
          className="bg-background px-3 py-2 font-mono"
        />
      </div>

      <div>
        <label className="text-xs text-muted mb-1 block">Description</label>
        <Input
          placeholder="What this skill does and when to use it..."
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          className="bg-background px-3 py-2"
        />
      </div>

      <div>
        <label className="text-xs text-muted mb-1 block">Instructions (Markdown)</label>
        <Textarea
          placeholder="Step-by-step instructions for the agent..."
          value={body}
          onChange={(e) => setBody(e.target.value)}
          rows={8}
          className="bg-background px-3 py-2 font-mono"
        />
      </div>

      {error && <p className="text-xs text-red-400">{error}</p>}

      <div className="flex gap-2">
        <button
          onClick={handleCreate}
          disabled={saving || !name.trim() || !description.trim()}
          className="px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-50 text-white text-xs font-medium"
        >
          {saving ? 'Creating...' : 'Create Skill'}
        </button>
        <button
          onClick={onCancel}
          className="px-3 py-1.5 rounded-lg text-muted hover:text-white text-xs"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}
