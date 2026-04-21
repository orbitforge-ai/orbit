import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { confirm } from '@tauri-apps/plugin-dialog';
import { ListChecks, Play, Pencil, Trash2, Zap } from 'lucide-react';
import { tasksApi } from '../../api/tasks';
import { projectsApi } from '../../api/projects';
import { Agent, Task } from '../../types';
import { useUiStore } from '../../store/uiStore';
import { ProjectPulseCard } from './ProjectPulseCard';
import { ProjectPulseEditor } from './ProjectPulseEditor';

const KIND_LABELS: Record<string, string> = {
  shell_command: 'Shell',
  script_file: 'Script',
  http_request: 'HTTP',
  agent_step: 'Agent Step',
  agent_loop: 'Agent Loop',
};

export function ProjectScheduledTab({ projectId }: { projectId: string }) {
  const queryClient = useQueryClient();
  const { navigate, editTask } = useUiStore();
  const [editingAgent, setEditingAgent] = useState<Agent | null>(null);

  const { data: allTasks = [], isLoading: tasksLoading } = useQuery<Task[]>({
    queryKey: ['tasks'],
    queryFn: tasksApi.list,
  });

  const { data: projectAgents = [], isLoading: agentsLoading } = useQuery<Agent[]>({
    queryKey: ['project-agents', projectId],
    queryFn: () => projectsApi.listAgents(projectId),
  });

  const tasks = allTasks.filter((t) => t.projectId === projectId && !t.tags.includes('pulse'));

  async function handleRun(taskId: string) {
    await tasksApi.trigger(taskId);
    queryClient.invalidateQueries({ queryKey: ['runs'] });
  }

  async function handleDelete(taskId: string) {
    if (!(await confirm('Delete this task?'))) return;
    await tasksApi.delete(taskId);
    queryClient.invalidateQueries({ queryKey: ['tasks'] });
  }

  if (tasksLoading || agentsLoading) {
    return (
      <div className="flex items-center justify-center h-32 text-muted text-sm">Loading…</div>
    );
  }

  return (
    <div className="flex flex-col h-full overflow-y-auto">
      {/* ── Pulses ─────────────────────────────────────────────────────── */}
      <section className="px-4 py-4 border-b border-edge">
        <div className="flex items-center justify-between mb-3">
          <h3 className="text-sm font-semibold text-white flex items-center gap-2">
            <Zap size={14} className="text-warning" />
            Pulses
            <span className="text-xs text-muted font-normal">({projectAgents.length})</span>
          </h3>
        </div>

        {projectAgents.length === 0 ? (
          <p className="text-xs text-muted">
            Assign agents to this project to configure per-project pulses.
          </p>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
            {projectAgents.map((agent) => (
              <ProjectPulseCard
                key={agent.id}
                agent={agent}
                projectId={projectId}
                onOpen={() => setEditingAgent(agent)}
              />
            ))}
          </div>
        )}
      </section>

      {/* ── Scheduled Tasks ────────────────────────────────────────────── */}
      <section className="flex-1 flex flex-col min-h-0">
        <div className="flex items-center justify-between px-4 py-3 border-b border-edge">
          <h3 className="text-sm font-semibold text-white">
            Scheduled Tasks
            <span className="ml-2 text-xs text-muted font-normal">({tasks.length})</span>
          </h3>
          <button
            onClick={() => navigate('task-builder')}
            className="px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover text-white text-xs font-medium transition-colors"
          >
            New Task
          </button>
        </div>

        {tasks.length === 0 ? (
          <div className="flex flex-col items-center justify-center flex-1 gap-3 text-muted py-10">
            <ListChecks size={32} className="opacity-30" />
            <p className="text-sm">No tasks in this project</p>
            <button
              onClick={() => navigate('task-builder')}
              className="px-4 py-2 rounded-lg bg-accent hover:bg-accent-hover text-white text-xs font-medium transition-colors"
            >
              Create Task
            </button>
          </div>
        ) : (
          <ul className="divide-y divide-edge">
            {tasks.map((task) => (
              <li
                key={task.id}
                className="group flex items-center gap-3 px-4 py-3 hover:bg-surface transition-colors"
              >
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-medium text-white truncate">{task.name}</span>
                    <span className="text-[10px] px-1.5 py-0.5 rounded bg-edge text-muted font-medium shrink-0">
                      {KIND_LABELS[task.kind] ?? task.kind}
                    </span>
                    {!task.enabled && (
                      <span className="text-[10px] px-1.5 py-0.5 rounded bg-red-400/10 text-red-400 font-medium shrink-0">
                        disabled
                      </span>
                    )}
                  </div>
                  {task.description && (
                    <p className="text-xs text-muted mt-0.5 truncate">{task.description}</p>
                  )}
                </div>

                <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                  <button
                    onClick={() => handleRun(task.id)}
                    disabled={!task.enabled}
                    className="p-1.5 rounded-md text-muted hover:text-green-400 hover:bg-green-400/10 disabled:opacity-30 transition-colors"
                    title="Run now"
                  >
                    <Play size={13} />
                  </button>
                  <button
                    onClick={() => editTask(task.id)}
                    className="p-1.5 rounded-md text-muted hover:text-white hover:bg-surface transition-colors"
                    title="Edit"
                  >
                    <Pencil size={13} />
                  </button>
                  <button
                    onClick={() => handleDelete(task.id)}
                    className="p-1.5 rounded-md text-muted hover:text-red-400 hover:bg-red-400/10 transition-colors"
                    title="Delete"
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>

      {editingAgent && (
        <ProjectPulseEditor
          agentId={editingAgent.id}
          agentName={editingAgent.name}
          projectId={projectId}
          open={!!editingAgent}
          onClose={() => setEditingAgent(null)}
        />
      )}
    </div>
  );
}
