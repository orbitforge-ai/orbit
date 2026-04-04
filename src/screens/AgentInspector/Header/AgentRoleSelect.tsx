import { Bot, Check, ChevronDown } from 'lucide-react';
import { AGENT_ROLES, DEFAULT_ROLE_ID, resolveRole } from '../../../lib/agentRoles';
import { ROLE_ICON_MAP } from '../RoleSelector';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import { AgentWorkspaceConfig } from '../../../types';

export function AgentRoleSelect({
  agentConfig,
  handleRoleChange,
}: {
  agentConfig: AgentWorkspaceConfig;
  handleRoleChange: (roleId: string) => void;
}) {
  const currentRole = resolveRole(agentConfig?.roleId);
  const CurrentRoleIcon = ROLE_ICON_MAP[currentRole.icon] ?? Bot;
  const isDefault = !agentConfig?.roleId || agentConfig.roleId === DEFAULT_ROLE_ID;
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button
          className={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] transition-[border-color,background-color] hover:border-edge-hover hover:bg-surface/60 ${isDefault ? 'border-edge bg-surface text-muted' : `border-edge bg-surface ${currentRole.color}`}`}
        >
          <CurrentRoleIcon size={10} />
          {currentRole.label}
          <ChevronDown size={9} className="opacity-60" />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          align="start"
          sideOffset={6}
          className="z-50 w-56 rounded-xl border border-edge bg-surface p-1.5 shadow-xl"
        >
          <p className="px-2 py-1 text-[10px] uppercase tracking-wide text-muted">Role</p>
          {AGENT_ROLES.map((role) => {
            const Icon = ROLE_ICON_MAP[role.icon] ?? Bot;
            const active = (agentConfig?.roleId ?? DEFAULT_ROLE_ID) === role.roleId;
            return (
              <DropdownMenu.Item
                key={role.roleId}
                onSelect={() => handleRoleChange(role.roleId)}
                className="flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm outline-none cursor-pointer hover:bg-accent/10 data-[highlighted]:bg-accent/10"
              >
                <Icon size={14} className={active ? 'text-accent-light' : role.color} />
                <span
                  className={`flex-1 ${active ? 'text-accent-light font-medium' : 'text-white'}`}
                >
                  {role.label}
                </span>
                {active && <Check size={12} className="text-accent-light" />}
              </DropdownMenu.Item>
            );
          })}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}
