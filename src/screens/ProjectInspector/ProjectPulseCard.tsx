import { useQuery, useQueryClient } from '@tanstack/react-query';
import * as Switch from '@radix-ui/react-switch';
import { Zap } from 'lucide-react';
import { pulseApi, PulseConfig } from '../../api/pulse';
import { humanSchedule } from '../../lib/humanSchedule';
import { Agent } from '../../types';
import { StatusBadge } from '../../components/StatusBadge';

interface ProjectPulseCardProps {
  agent: Agent;
  projectId: string;
  onOpen: () => void;
}

export function ProjectPulseCard({ agent, projectId, onOpen }: ProjectPulseCardProps) {
  const queryClient = useQueryClient();

  const { data: pulseConfig } = useQuery<PulseConfig>({
    queryKey: ['pulse-config', agent.id, projectId],
    queryFn: () => pulseApi.getConfig(agent.id, projectId),
  });

  const isConfigured = !!pulseConfig?.taskId;
  const canToggle = isConfigured && !!pulseConfig?.schedule;

  async function handleToggle(next: boolean) {
    if (!canToggle || !pulseConfig?.schedule) {
      onOpen();
      return;
    }
    try {
      await pulseApi.update(
        agent.id,
        projectId,
        pulseConfig.content,
        pulseConfig.schedule,
        next
      );
      queryClient.invalidateQueries({ queryKey: ['pulse-config', agent.id, projectId] });
      queryClient.invalidateQueries({ queryKey: ['schedules'] });
      queryClient.invalidateQueries({ queryKey: ['tasks'] });
    } catch (err) {
      console.error('Failed to toggle pulse:', err);
    }
  }

  const summary = isConfigured
    ? pulseConfig?.schedule
      ? humanSchedule(pulseConfig.schedule)
      : 'Configured'
    : 'Not configured';

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onOpen}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onOpen();
        }
      }}
      className="group flex flex-col gap-2 p-3 rounded-lg border border-edge bg-panel hover:border-accent hover:bg-accent/5 transition-colors text-left cursor-pointer"
    >
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0">
          <Zap
            size={14}
            className={pulseConfig?.enabled ? 'text-warning shrink-0' : 'text-muted shrink-0'}
          />
          <div className="min-w-0">
            <p className="text-sm font-medium text-white truncate">{agent.name}</p>
            <div className="flex items-center gap-1.5 mt-0.5">
              <StatusBadge state={agent.state} />
            </div>
          </div>
        </div>

        <div
          className="flex items-center gap-2 shrink-0"
          onClick={(e) => e.stopPropagation()}
        >
          <span
            className={`text-[10px] font-medium ${
              pulseConfig?.enabled ? 'text-emerald-400' : 'text-muted'
            }`}
          >
            {pulseConfig?.enabled ? 'Active' : 'Inactive'}
          </span>
          <Switch.Root
            checked={!!pulseConfig?.enabled}
            onCheckedChange={handleToggle}
            className={`w-9 h-5 rounded-full bg-edge data-[state=checked]:bg-emerald-500 transition-colors outline-none ${
              canToggle ? '' : 'opacity-60'
            }`}
            title={canToggle ? undefined : 'Configure a prompt and schedule first'}
          >
            <Switch.Thumb className="block w-4 h-4 rounded-full bg-white shadow translate-x-0.5 data-[state=checked]:translate-x-[18px] transition-transform" />
          </Switch.Root>
        </div>
      </div>

      <p className="text-xs text-muted truncate">{summary}</p>
      {pulseConfig?.enabled && pulseConfig?.nextRunAt && (
        <p className="text-[10px] text-border-hover">
          Next: {new Date(pulseConfig.nextRunAt).toLocaleString()}
        </p>
      )}
    </div>
  );
}
