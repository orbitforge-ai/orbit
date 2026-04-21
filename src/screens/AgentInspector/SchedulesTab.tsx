import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Trash2 } from 'lucide-react';
import * as Switch from '@radix-ui/react-switch';
import { invoke } from '@tauri-apps/api/core';
import { schedulesApi } from '../../api/schedules';
import { humanSchedule } from '../../lib/humanSchedule';
import { RecurringConfig, Schedule, Task } from '../../types';

interface SchedulesTabProps {
  agentId: string;
}

export function SchedulesTab({ agentId }: SchedulesTabProps) {
  return (
    <div className="p-6 space-y-8 h-full overflow-y-auto">
      <AgentSchedulesList agentId={agentId} />
    </div>
  );
}

function AgentSchedulesList({ agentId }: { agentId: string }) {
  const queryClient = useQueryClient();

  const { data: tasks = [] } = useQuery<Task[]>({
    queryKey: ['tasks'],
    queryFn: () => invoke('list_tasks'),
  });

  const { data: allSchedules = [] } = useQuery<Schedule[]>({
    queryKey: ['schedules'],
    queryFn: schedulesApi.list,
    refetchInterval: 10_000,
  });

  const agentTaskIds = new Set(
    tasks.filter((t) => t.agentId === agentId && !t.tags.includes('pulse')).map((t) => t.id)
  );

  const agentSchedules = allSchedules.filter((s) => s.taskId && agentTaskIds.has(s.taskId));

  async function handleToggle(schedule: Schedule) {
    await schedulesApi.toggle(schedule.id, !schedule.enabled);
    queryClient.invalidateQueries({ queryKey: ['schedules'] });
  }

  async function handleDelete(schedule: Schedule) {
    await schedulesApi.delete(schedule.id);
    queryClient.invalidateQueries({ queryKey: ['schedules'] });
  }

  function getTaskName(taskId: string): string {
    return tasks.find((t) => t.id === taskId)?.name ?? 'Unknown Task';
  }

  return (
    <section className="space-y-3">
      <h4 className="text-sm font-semibold text-white">Task Schedules</h4>

      {agentSchedules.length === 0 ? (
        <p className="text-xs text-muted">
          No schedules for this agent's tasks. Create one from the Schedules screen.
        </p>
      ) : (
        <div className="space-y-2">
          {agentSchedules.map((schedule) => {
            const config = schedule.config as RecurringConfig | null;
            const description = config ? humanSchedule(config) : schedule.kind;

            return (
              <div
                key={schedule.id}
                className="flex items-center gap-3 px-4 py-3 rounded-lg border border-edge bg-surface"
              >
                <div className="flex-1 min-w-0">
                  <p className="text-sm text-white truncate">
                    {schedule.taskId ? getTaskName(schedule.taskId) : 'Workflow schedule'}
                  </p>
                  <p className="text-xs text-muted">{description}</p>
                  {schedule.nextRunAt && (
                    <p className="text-[10px] text-border-hover mt-0.5">
                      Next: {new Date(schedule.nextRunAt).toLocaleString()}
                    </p>
                  )}
                </div>

                <Switch.Root
                  checked={schedule.enabled}
                  onCheckedChange={() => handleToggle(schedule)}
                  className="w-9 h-5 rounded-full bg-edge data-[state=checked]:bg-emerald-500 transition-colors outline-none shrink-0"
                >
                  <Switch.Thumb className="block w-4 h-4 rounded-full bg-white shadow translate-x-0.5 data-[state=checked]:translate-x-[18px] transition-transform" />
                </Switch.Root>

                <button
                  onClick={() => handleDelete(schedule)}
                  className="p-1.5 rounded text-muted hover:text-red-400 hover:bg-red-500/10 transition-colors shrink-0"
                >
                  <Trash2 size={13} />
                </button>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
