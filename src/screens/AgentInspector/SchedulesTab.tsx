import { useEffect, useState, useImperativeHandle, forwardRef } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Zap, Clock, Trash2, ExternalLink, Play } from 'lucide-react';
import * as Switch from '@radix-ui/react-switch';
import { invoke } from '@tauri-apps/api/core';
import { tasksApi } from '../../api/tasks';
import { pulseApi, PulseConfig } from '../../api/pulse';
import { schedulesApi } from '../../api/schedules';
import { RecurringPicker } from '../ScheduleBuilder/RecurringPicker';
import { humanSchedule } from '../../lib/humanSchedule';
import { RecurringConfig, Schedule, Task } from '../../types';
import { useUiStore } from '../../store/uiStore';

const DEFAULT_SCHEDULE: RecurringConfig = {
  intervalUnit: 'hours',
  intervalValue: 1,
  timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
  missedRunPolicy: 'skip' as const,
};

interface SchedulesTabProps {
  agentId: string;
  onDirtyChange?: (dirty: boolean) => void;
}

export const SchedulesTab = forwardRef<{ triggerSave: () => void }, SchedulesTabProps>(
  function SchedulesTab({ agentId, onDirtyChange }, ref) {
    return (
      <div className="p-6 space-y-8 h-full overflow-y-auto">
        <PulseSection agentId={agentId} onDirtyChange={onDirtyChange} ref={ref} />
        <div className="border-t border-edge" />
        <AgentSchedulesList agentId={agentId} />
      </div>
    );
  }
);

// ─── Pulse Section ──────────────────────────────────────────────────────────

interface PulseSectionProps {
  agentId: string;
  onDirtyChange?: (dirty: boolean) => void;
}

const PulseSection = forwardRef<{ triggerSave: () => void }, PulseSectionProps>(
  function PulseSection({ agentId, onDirtyChange }, ref) {
    const queryClient = useQueryClient();
    const { navigate } = useUiStore();
    const [, setSaving] = useState(false);
    const [, setSaved] = useState(false);
    const [triggering, setTriggering] = useState(false);
    const [content, setContent] = useState('');
    const [schedule, setSchedule] = useState<RecurringConfig>(DEFAULT_SCHEDULE);
    const [enabled, setEnabled] = useState(false);
    const [, setIsDirty] = useState(false);

    // Expose triggerSave via ref
    useImperativeHandle(ref, () => ({
      triggerSave: () => handleSave(),
    }));

    function markDirty() {
      setIsDirty(true);
      onDirtyChange?.(true);
    }

    function markClean() {
      setIsDirty(false);
      onDirtyChange?.(false);
    }

    const { data: pulseConfig } = useQuery<PulseConfig>({
      queryKey: ['pulse-config', agentId],
      queryFn: () => pulseApi.getConfig(agentId),
    });

    // Sync local state from loaded config
    useEffect(() => {
      if (pulseConfig) {
        setContent(pulseConfig.content);
        setEnabled(pulseConfig.enabled);
        if (pulseConfig.schedule) {
          setSchedule(pulseConfig.schedule);
        }
        markClean();
      }
    }, [pulseConfig]);

    async function handleSave() {
      setSaving(true);
      setSaved(false);
      try {
        await pulseApi.update(agentId, content, schedule, enabled);
        queryClient.invalidateQueries({ queryKey: ['pulse-config', agentId] });
        queryClient.invalidateQueries({ queryKey: ['schedules'] });
        setSaved(true);
        markClean();
        setTimeout(() => setSaved(false), 2000);
      } catch (err) {
        console.error('Failed to save pulse:', err);
      }
      setSaving(false);
    }

    return (
      <section className="space-y-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Zap size={16} className="text-warning" />
            <h4 className="text-sm font-semibold text-white">Pulse</h4>
          </div>

          {/* Enable/disable toggle */}
          <div className="flex items-center gap-2">
            <span className={`text-xs ${enabled ? 'text-emerald-400' : 'text-muted'}`}>
              {enabled ? 'Active' : 'Inactive'}
            </span>
            <Switch.Root
              checked={enabled}
              onCheckedChange={(v) => {
                setEnabled(v);
                markDirty();
              }}
              className="w-9 h-5 rounded-full bg-edge data-[state=checked]:bg-emerald-500 transition-colors outline-none"
            >
              <Switch.Thumb className="block w-4 h-4 rounded-full bg-white shadow translate-x-0.5 data-[state=checked]:translate-x-[18px] transition-transform" />
            </Switch.Root>
          </div>
        </div>

        <p className="text-xs text-muted">
          Define a recurring prompt that runs automatically on a schedule. All responses are logged
          to a dedicated Pulse chat session.
        </p>

        {/* Pulse content editor */}
        <div>
          <label className="text-xs text-muted mb-1 block">Prompt</label>
          <textarea
            value={content}
            onChange={(e) => {
              setContent(e.target.value);
              markDirty();
            }}
            rows={6}
            className="w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm font-mono resize-y focus:outline-none focus:border-accent leading-relaxed"
            placeholder="Describe what this agent should do on each pulse..."
          />
        </div>

        {/* Schedule picker */}
        <div>
          <label className="text-xs text-muted mb-1 block">Frequency</label>
          <RecurringPicker
            value={schedule}
            onChange={(s) => {
              setSchedule(s);
              markDirty();
            }}
          />
        </div>

        {/* Status info */}
        {pulseConfig?.nextRunAt && enabled && (
          <div className="flex items-center gap-2 text-xs text-muted">
            <Clock size={11} />
            <span>Next run: {new Date(pulseConfig.nextRunAt).toLocaleString()}</span>
          </div>
        )}
        {pulseConfig?.lastRunAt && (
          <div className="flex items-center gap-2 text-xs text-muted">
            <Clock size={11} />
            <span>Last run: {new Date(pulseConfig.lastRunAt).toLocaleString()}</span>
          </div>
        )}

        {/* Actions */}
        <div className="flex items-center gap-3">
          {pulseConfig?.taskId && (
            <button
              onClick={async () => {
                if (!pulseConfig?.taskId) return;
                setTriggering(true);
                try {
                  await tasksApi.trigger(pulseConfig.taskId);
                  queryClient.invalidateQueries({ queryKey: ['pulse-config', agentId] });
                } catch (err) {
                  console.error('Failed to trigger pulse:', err);
                }
                setTriggering(false);
              }}
              disabled={triggering}
              className="flex items-center gap-1.5 px-3 py-2 rounded-lg border border-edge text-secondary hover:text-white hover:border-edge-hover disabled:opacity-50 text-xs transition-colors"
            >
              <Play size={12} />
              {triggering ? 'Running...' : 'Run Now'}
            </button>
          )}

          {pulseConfig?.sessionId && (
            <button
              onClick={() => navigate('chat')}
              className="flex items-center gap-1.5 px-3 py-2 rounded-lg border border-edge text-secondary hover:text-white hover:border-edge-hover text-xs transition-colors"
            >
              <ExternalLink size={12} />
              View Pulse Log
            </button>
          )}
        </div>
      </section>
    );
  }
);

// ─── Other Agent Schedules ──────────────────────────────────────────────────

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

  // Filter: tasks for this agent, then schedules for those tasks, excluding pulse
  const agentTaskIds = new Set(
    tasks.filter((t) => t.agentId === agentId && !t.tags.includes('pulse')).map((t) => t.id)
  );

  const agentSchedules = allSchedules.filter((s) => agentTaskIds.has(s.taskId));

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
                  <p className="text-sm text-white truncate">{getTaskName(schedule.taskId)}</p>
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
