import { useState, useEffect } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Check, ChevronLeft } from 'lucide-react';
import { tasksApi } from '../../api/tasks';
import { useUiStore } from '../../store/uiStore';
import { ShellCommandConfig } from '../../types';

export function TaskEdit() {
  const { editingTaskId, navigate } = useUiStore();
  const queryClient = useQueryClient();

  const { data: task, isLoading } = useQuery({
    queryKey: ['tasks', editingTaskId],
    queryFn: () => tasksApi.get(editingTaskId!),
    enabled: !!editingTaskId,
  });

  const [name, setName] = useState('');
  const [command, setCommand] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Populate form once task loads
  useEffect(() => {
    if (!task) return;
    setName(task.name);
    const cfg = task.config as ShellCommandConfig;
    setCommand(cfg.command ?? '');
  }, [task]);

  const canSave = name.trim().length > 0 && command.trim().length > 0;

  async function handleSave() {
    if (!editingTaskId) return;
    setSaving(true);
    setError(null);
    try {
      const config: ShellCommandConfig = { command };
      await tasksApi.update(editingTaskId, { name, config });
      queryClient.invalidateQueries({ queryKey: ['tasks'] });
      navigate('tasks');
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full text-muted text-sm">Loading…</div>
    );
  }

  if (!task) {
    return (
      <div className="flex items-center justify-center h-full text-muted text-sm">
        Task not found.
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full max-w-2xl mx-auto p-6">
      <div className="mb-8">
        <button
          onClick={() => navigate('tasks')}
          className="flex items-center gap-1.5 text-sm text-muted hover:text-white mb-4 transition-colors"
        >
          <ChevronLeft size={14} />
          Back to Tasks
        </button>
        <h2 className="text-xl font-semibold text-white">Edit Task</h2>
      </div>

      <div className="flex-1 overflow-y-auto space-y-5">
        <div>
          <label className="block text-sm font-medium text-secondary mb-1.5">Task name</label>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="w-full px-4 py-2.5 rounded-lg bg-surface border border-edge text-white text-sm placeholder-border-hover focus:outline-none focus:border-accent"
          />
        </div>

        <div>
          <label className="block text-sm font-medium text-secondary mb-1.5">Command</label>
          <textarea
            value={command}
            onChange={(e) => setCommand(e.target.value)}
            rows={8}
            className="w-full px-4 py-3 rounded-lg bg-inset border border-edge text-green-400 text-sm font-mono placeholder-border focus:outline-none focus:border-accent resize-none"
          />
        </div>

        {error && (
          <div className="px-4 py-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">
            {error}
          </div>
        )}
      </div>

      <div className="flex items-center justify-end pt-6 border-t border-edge mt-6">
        <button
          disabled={!canSave || saving}
          onClick={handleSave}
          className="flex items-center gap-2 px-4 py-2 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-50 disabled:cursor-not-allowed text-white text-sm font-medium transition-colors"
        >
          {saving ? 'Saving…' : 'Save Changes'}
          <Check size={14} />
        </button>
      </div>
    </div>
  );
}
