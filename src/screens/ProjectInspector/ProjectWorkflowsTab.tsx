import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { confirm } from '@tauri-apps/plugin-dialog';
import { Pencil, Plus, Trash2, Workflow as WorkflowIcon } from 'lucide-react';
import { projectWorkflowsApi } from '../../api/projectWorkflows';
import { ProjectWorkflow } from '../../types';
import { useUiStore } from '../../store/uiStore';

export function ProjectWorkflowsTab({ projectId }: { projectId: string }) {
  const queryClient = useQueryClient();
  const { openWorkflowEditor } = useUiStore();
  const [showCreate, setShowCreate] = useState(false);

  const { data: workflows = [], isLoading } = useQuery<ProjectWorkflow[]>({
    queryKey: ['project-workflows', projectId],
    queryFn: () => projectWorkflowsApi.list(projectId),
  });

  const createMutation = useMutation({
    mutationFn: (name: string) =>
      projectWorkflowsApi.create({ projectId, name }),
    onSuccess: (workflow) => {
      queryClient.invalidateQueries({ queryKey: ['project-workflows', projectId] });
      setShowCreate(false);
      openWorkflowEditor(workflow.id);
    },
  });

  const enableMutation = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      projectWorkflowsApi.setEnabled(id, enabled),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: ['project-workflows', projectId] }),
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => projectWorkflowsApi.delete(id),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: ['project-workflows', projectId] }),
  });

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-32 text-muted text-sm">Loading…</div>
    );
  }

  return (
    <div className="flex flex-col h-full overflow-y-auto">
      <div className="flex items-center justify-between px-4 py-3 border-b border-edge">
        <h3 className="text-sm font-semibold text-white">
          Workflows
          <span className="ml-2 text-xs text-muted font-normal">({workflows.length})</span>
        </h3>
        <button
          onClick={() => setShowCreate(true)}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover text-white text-xs font-medium transition-colors"
        >
          <Plus size={12} />
          New Workflow
        </button>
      </div>

      {showCreate && (
        <CreateWorkflowForm
          onCreate={(name) => createMutation.mutate(name)}
          onCancel={() => setShowCreate(false)}
          creating={createMutation.isPending}
        />
      )}

      {workflows.length === 0 && !showCreate ? (
        <div className="flex flex-col items-center justify-center flex-1 gap-3 text-muted p-6">
          <WorkflowIcon size={32} className="opacity-30" />
          <p className="text-sm text-center max-w-sm">
            Workflows let agents run multi-step automations on a trigger. Drag nodes onto a canvas
            to compose flows like &ldquo;new email → categorize → reply or file&rdquo;.
          </p>
          <button
            onClick={() => setShowCreate(true)}
            className="px-4 py-2 rounded-lg bg-accent hover:bg-accent-hover text-white text-xs font-medium transition-colors"
          >
            Create your first workflow
          </button>
        </div>
      ) : (
        <ul className="divide-y divide-edge">
          {workflows.map((workflow) => (
            <li
              key={workflow.id}
              className="group flex items-center gap-3 px-4 py-3 hover:bg-surface transition-colors"
            >
              <div className="w-8 h-8 rounded-lg bg-accent/15 flex items-center justify-center shrink-0">
                <WorkflowIcon size={14} className="text-accent-hover" />
              </div>
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <p className="text-sm font-medium text-white truncate">{workflow.name}</p>
                  <span className="text-[10px] uppercase tracking-wider text-muted font-mono">
                    v{workflow.version}
                  </span>
                  <span
                    className={
                      'text-[10px] uppercase tracking-wider px-1.5 py-0.5 rounded font-mono ' +
                      (workflow.enabled
                        ? 'bg-emerald-500/15 text-emerald-300'
                        : 'bg-muted/15 text-muted')
                    }
                  >
                    {workflow.enabled ? 'enabled' : 'draft'}
                  </span>
                </div>
                {workflow.description && (
                  <p className="text-xs text-muted mt-0.5 truncate">{workflow.description}</p>
                )}
                <p className="text-[11px] text-muted mt-0.5 font-mono">
                  trigger: {workflow.triggerKind} · {workflow.graph.nodes.length} nodes
                </p>
              </div>
              <label className="flex items-center gap-1.5 text-xs text-muted cursor-pointer">
                <input
                  type="checkbox"
                  checked={workflow.enabled}
                  onChange={(e) =>
                    enableMutation.mutate({ id: workflow.id, enabled: e.target.checked })
                  }
                  className="accent-accent"
                />
                Enabled
              </label>
              <button
                onClick={() => openWorkflowEditor(workflow.id)}
                className="p-1.5 rounded-md text-muted hover:text-white hover:bg-surface transition-colors"
                title="Open editor"
              >
                <Pencil size={14} />
              </button>
              <button
                onClick={async () => {
                  if (!(await confirm(`Delete workflow "${workflow.name}"?`))) return;
                  deleteMutation.mutate(workflow.id);
                }}
                className="p-1.5 rounded-md text-muted hover:text-red-400 hover:bg-red-400/10 transition-colors"
                title="Delete workflow"
              >
                <Trash2 size={14} />
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function CreateWorkflowForm({
  onCreate,
  onCancel,
  creating,
}: {
  onCreate: (name: string) => void;
  onCancel: () => void;
  creating: boolean;
}) {
  const [name, setName] = useState('');
  return (
    <div className="mx-4 my-3 p-3 rounded-xl border border-edge bg-surface space-y-2">
      <input
        autoFocus
        value={name}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && name.trim()) onCreate(name.trim());
          if (e.key === 'Escape') onCancel();
        }}
        placeholder="Workflow name"
        className="w-full bg-background border border-edge rounded-lg px-3 py-2 text-sm text-white placeholder-muted outline-none focus:border-accent"
      />
      <div className="flex gap-2">
        <button
          onClick={() => name.trim() && onCreate(name.trim())}
          disabled={!name.trim() || creating}
          className="px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-50 text-white text-xs font-medium transition-colors"
        >
          {creating ? 'Creating…' : 'Create'}
        </button>
        <button
          onClick={onCancel}
          className="px-3 py-1.5 rounded-lg border border-edge text-muted hover:text-white text-xs transition-colors"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}
