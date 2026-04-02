import { Bot, Code2, Bug, Search, Share2, BarChart2, LucideIcon } from 'lucide-react';
import { AGENT_ROLES, AgentRole, DEFAULT_ROLE_ID, resolveRole } from '../../lib/agentRoles';

export const ROLE_ICON_MAP: Record<string, LucideIcon> = {
  Bot,
  Code2,
  Bug,
  Search,
  Share2,
  BarChart2,
};

interface RoleSelectorProps {
  selected: string | undefined;
  onSelect: (roleId: string) => void;
  mode?: 'full' | 'compact';
}

export function RoleSelector({ selected, onSelect, mode = 'full' }: RoleSelectorProps) {
  const selectedId = selected ?? DEFAULT_ROLE_ID;

  if (mode === 'compact') {
    return (
      <div className="grid grid-cols-3 gap-2">
        {AGENT_ROLES.map((role) => (
          <RoleCard key={role.roleId} role={role} selected={selectedId === role.roleId} onSelect={onSelect} compact />
        ))}
      </div>
    );
  }

  return (
    <div className="grid grid-cols-2 gap-2">
      {AGENT_ROLES.map((role) => (
        <RoleCard key={role.roleId} role={role} selected={selectedId === role.roleId} onSelect={onSelect} />
      ))}
    </div>
  );
}

interface RoleCardProps {
  role: AgentRole;
  selected: boolean;
  onSelect: (roleId: string) => void;
  compact?: boolean;
}

function RoleCard({ role, selected, onSelect, compact = false }: RoleCardProps) {
  const Icon = ROLE_ICON_MAP[role.icon] ?? Bot;

  if (compact) {
    return (
      <button
        onClick={() => onSelect(role.roleId)}
        className={`flex flex-col items-center gap-1.5 px-2 py-2.5 rounded-lg border text-center transition-colors ${
          selected
            ? 'border-accent bg-accent/10'
            : 'border-edge bg-surface hover:border-edge-hover'
        }`}
      >
        <Icon
          size={16}
          className={selected ? 'text-accent-light' : role.color}
        />
        <span
          className={`text-[11px] font-medium leading-tight ${selected ? 'text-accent-light' : 'text-white'}`}
        >
          {role.label}
        </span>
      </button>
    );
  }

  return (
    <button
      onClick={() => onSelect(role.roleId)}
      className={`flex items-start gap-3 px-3 py-3 rounded-lg border text-left transition-colors ${
        selected
          ? 'border-accent bg-accent/10'
          : 'border-edge bg-surface hover:border-edge-hover'
      }`}
    >
      <Icon
        size={18}
        className={`mt-0.5 shrink-0 ${selected ? 'text-accent-light' : role.color}`}
      />
      <div className="min-w-0">
        <span
          className={`text-sm font-medium block ${selected ? 'text-accent-light' : 'text-white'}`}
        >
          {role.label}
        </span>
        <span className="text-[11px] text-muted leading-snug block mt-0.5">
          {role.description}
        </span>
      </div>
    </button>
  );
}

interface RoleBadgeProps {
  roleId: string;
}

export function RoleBadge({ roleId }: RoleBadgeProps) {
  const role = resolveRole(roleId);
  const Icon = ROLE_ICON_MAP[role.icon] ?? Bot;

  return (
    <div
      className={`inline-flex items-center gap-1.5 rounded-full border border-edge bg-surface px-2.5 py-1 text-[11px] ${role.color}`}
    >
      <Icon size={10} />
      {role.label}
    </div>
  );
}
