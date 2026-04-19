import { useMemo } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Play, Pencil, Trash2, ToggleLeft, ToggleRight, Clock } from 'lucide-react';
import { tasksApi } from '../../api/tasks';
import { schedulesApi } from '../../api/schedules';
import { runsApi } from '../../api/runs';
import { projectsApi } from '../../api/projects';
import { StatusBadge } from '../../components/StatusBadge';
import { useUiStore } from '../../store/uiStore';
import { KIND_OPTIONS } from '../../lib/taskConstants';
import { humanSchedule } from '../../lib/humanSchedule';
import type { Task, Schedule, RunSummary, RecurringConfig } from '../../types';
import { info } from '@tauri-apps/plugin-log';
import { confirm } from '@tauri-apps/plugin-dialog';

export function TasksScreen() {
  const { navigate, editTask } = useUiStore();
  const queryClient = useQueryClient();

  const { data: tasks = [], isLoading } = useQuery({
    queryKey: ['tasks'],
    queryFn: tasksApi.list,
    select: (all: Task[]) => all.filter((t) => !t.tags.includes('pulse')),
  });

  const { data: schedules = [] } = useQuery({
    queryKey: ['schedules'],
    queryFn: schedulesApi.list,
  });

  const { data: recentRuns = [] } = useQuery({
    queryKey: ['runs', 'recent-for-tasks'],
    queryFn: () => runsApi.list({ limit: 100 }),
  });

  const { data: projects = [] } = useQuery({
    queryKey: ['projects'],
    queryFn: projectsApi.list,
  });

  // Index: taskId -> first matching schedule
  const scheduleByTask = useMemo(() => {
    const map = new Map<string, Schedule>();
    for (const s of schedules) {
      if (s.taskId && !map.has(s.taskId)) map.set(s.taskId, s);
    }
    return map;
  }, [schedules]);

  // Index: taskId -> most recent run
  const lastRunByTask = useMemo(() => {
    const map = new Map<string, RunSummary>();
    for (const r of recentRuns) {
      if (r.taskId && !map.has(r.taskId)) map.set(r.taskId, r);
    }
    return map;
  }, [recentRuns]);

  async function handleTrigger(task: Task) {
    console.log(`Triggering task: ${task.name}`);
    await tasksApi.trigger(task.id);
    queryClient.invalidateQueries({ queryKey: ['runs'] });
    navigate('history');
  }

  async function handleToggle(task: Task) {
    info(`${!task.enabled ? 'Enabling' : 'Disabling'} task: ${task.name}`);
    await tasksApi.update(task.id, { enabled: !task.enabled });
    queryClient.invalidateQueries({ queryKey: ['tasks'] });
  }

  async function handleDelete(task: Task) {
    if (
      !(await confirm(`Are you sure you want to delete "${task.name}"?`, {
        title: 'Confirm Delete',
      }))
    )
      return;
    await tasksApi.delete(task.id);
    queryClient.invalidateQueries({ queryKey: ['tasks'] });
  }

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center justify-between px-6 py-4 border-b border-edge">
        <h2 className="text-lg font-semibold text-white">Tasks</h2>
        <button
          onClick={() => navigate('task-builder')}
          className="px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover text-white text-sm font-medium transition-colors"
        >
          + New Task
        </button>
      </div>

      <div className="flex-1 overflow-y-auto">
        {isLoading && <div className="p-8 text-center text-muted text-sm">Loading…</div>}
        {!isLoading && tasks.length === 0 && (
          <div className="p-16 text-center">
            <p className="text-muted text-sm">No tasks yet</p>
            <button
              onClick={() => navigate('task-builder')}
              className="mt-3 px-4 py-2 rounded-lg bg-accent text-white text-sm"
            >
              Create your first task
            </button>
          </div>
        )}

        <div className="divide-y divide-border">
          {tasks.map((task) => {
            const kindInfo = KIND_OPTIONS.find((k) => k.id === task.kind);
            const KindIcon = kindInfo?.icon;
            const sched = scheduleByTask.get(task.id);
            const lastRun = lastRunByTask.get(task.id);
            const schedLabel = sched
              ? sched.kind === 'recurring'
                ? humanSchedule(sched.config as RecurringConfig)
                : 'One-shot'
              : null;

            return (
              <div
                key={task.id}
                className={`flex items-center gap-3 px-6 py-4 hover:bg-surface transition-colors ${
                  !task.enabled ? 'opacity-50' : ''
                }`}
              >
                {/* Kind icon */}
                {KindIcon && (
                  <div className="shrink-0 w-8 h-8 rounded-lg bg-surface flex items-center justify-center">
                    <KindIcon size={15} className="text-muted" />
                  </div>
                )}

                {/* Name + description + meta */}
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <p className="text-sm font-medium text-white truncate">{task.name}</p>
                    {task.projectId && (
                      <span className="px-1.5 py-0.5 rounded bg-accent/20 border border-accent/30 text-accent-light text-[10px] font-medium shrink-0">
                        {projects.find((p) => p.id === task.projectId)?.name ?? 'Project'}
                      </span>
                    )}
                    {!task.enabled && <StatusBadge state="cancelled" />}
                  </div>
                  {task.description && (
                    <p className="text-xs text-muted mt-0.5 truncate">{task.description}</p>
                  )}
                  <div className="flex items-center gap-3 mt-1">
                    <span className="text-xs text-muted capitalize">
                      {kindInfo?.label ?? task.kind.replace('_', ' ')}
                    </span>
                    {schedLabel && (
                      <span className="flex items-center gap-1 text-xs text-muted">
                        <Clock size={10} />
                        {schedLabel}
                      </span>
                    )}
                    {lastRun && (
                      <StatusBadge state={lastRun.state} />
                    )}
                  </div>
                </div>

                {/* Actions */}
                <div className="flex items-center gap-1 shrink-0">
                  <button
                    onClick={() => handleTrigger(task)}
                    title="Run now"
                    className="p-1.5 rounded text-muted hover:text-green-400 hover:bg-green-500/10 transition-colors"
                  >
                    <Play size={14} />
                  </button>
                  <button
                    onClick={() => editTask(task.id)}
                    title="Edit"
                    className="p-1.5 rounded text-muted hover:text-white hover:bg-edge transition-colors"
                  >
                    <Pencil size={14} />
                  </button>
                  <button
                    onClick={() => handleToggle(task)}
                    title={task.enabled ? 'Disable' : 'Enable'}
                    className="p-1.5 rounded text-muted hover:text-white hover:bg-edge transition-colors"
                  >
                    {task.enabled ? <ToggleRight size={14} /> : <ToggleLeft size={14} />}
                  </button>
                  <button
                    type="button"
                    onClick={() => handleDelete(task)}
                    title="Delete"
                    className="p-1.5 rounded text-muted hover:text-red-400 hover:bg-red-500/10 transition-colors"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
