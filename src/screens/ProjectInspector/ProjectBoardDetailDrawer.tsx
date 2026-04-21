import { useEffect, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { confirm } from '@tauri-apps/plugin-dialog';
import { Bot, CheckCircle, Trash2, X } from 'lucide-react';
import { workItemsApi } from '../../api/workItems';
import { projectsApi } from '../../api/projects';
import { Agent, ProjectBoardColumn, WorkItem, WorkItemKind, WorkItemStatus } from '../../types';
import { Input, SimpleSelect, Textarea } from '../../components/ui';
import { WorkItemComments } from './WorkItemComments';

const KIND_OPTIONS: { id: WorkItemKind; label: string }[] = [
  { id: 'task', label: 'Task' },
  { id: 'bug', label: 'Bug' },
  { id: 'story', label: 'Story' },
  { id: 'spike', label: 'Spike' },
  { id: 'chore', label: 'Chore' },
];

export function ProjectBoardDetailDrawer({
  projectId,
  workItemId,
  agents,
  onClose,
}: {
  projectId: string;
  workItemId: string;
  agents: Agent[];
  onClose: () => void;
}) {
  const queryClient = useQueryClient();

  const { data: item, isLoading } = useQuery<WorkItem>({
    queryKey: ['work-items', projectId, workItemId],
    queryFn: () => workItemsApi.get(workItemId),
  });
  const { data: columns = [] } = useQuery<ProjectBoardColumn[]>({
    queryKey: ['project-board-columns', projectId],
    queryFn: () => projectsApi.listBoardColumns(projectId),
  });

  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [labelsInput, setLabelsInput] = useState('');
  const [dirty, setDirty] = useState(false);

  useEffect(() => {
    if (item) {
      setTitle(item.title);
      setDescription(item.description ?? '');
      setLabelsInput(item.labels.join(', '));
      setDirty(false);
    }
  }, [item?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  function invalidate() {
    queryClient.invalidateQueries({ queryKey: ['work-items', projectId] });
    queryClient.invalidateQueries({ queryKey: ['work-items', projectId, workItemId] });
  }

  const updateMutation = useMutation({
    mutationFn: (payload: Parameters<typeof workItemsApi.update>[1]) =>
      workItemsApi.update(workItemId, payload),
    onSuccess: () => {
      invalidate();
      setDirty(false);
    },
  });

  const moveMutation = useMutation({
    mutationFn: ({ columnId, status }: { columnId: string; status?: WorkItemStatus }) =>
      workItemsApi.move(workItemId, status, columnId),
    onSuccess: invalidate,
  });

  const blockMutation = useMutation({
    mutationFn: ({ reason }: { reason: string }) =>
      workItemsApi.block(workItemId, reason),
    onSuccess: invalidate,
  });

  const completeMutation = useMutation({
    mutationFn: () => workItemsApi.complete(workItemId),
    onSuccess: invalidate,
  });

  const claimMutation = useMutation({
    mutationFn: ({ agentId }: { agentId: string }) =>
      workItemsApi.claim(workItemId, agentId),
    onSuccess: invalidate,
  });

  const deleteMutation = useMutation({
    mutationFn: () => workItemsApi.delete(workItemId),
    onSuccess: () => {
      invalidate();
      onClose();
    },
  });

  function handleSave() {
    const labels = labelsInput
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);
    updateMutation.mutate({ title, description, labels });
  }

  function handleColumnChange(nextColumnId: string) {
    const nextColumn = columns.find((column) => column.id === nextColumnId);
    if (!nextColumn) return;
    if (nextColumn.role === 'blocked') {
      const reason = window.prompt('Why is this card blocked?');
      if (!reason || !reason.trim()) return;
      blockMutation.mutate({ reason: reason.trim() });
      return;
    }
    if (nextColumn.role === 'done') {
      completeMutation.mutate();
      return;
    }
    moveMutation.mutate({ columnId: nextColumnId, status: nextColumn.role ?? undefined });
  }

  function handleAssigneeChange(agentId: string) {
    if (agentId) {
      claimMutation.mutate({ agentId });
    } else {
      updateMutation.mutate({}); // no-op; clearing assignee not supported via update — leave to claim
    }
  }

  function handleKindChange(kind: WorkItemKind) {
    updateMutation.mutate({ kind });
  }

  function handlePriorityChange(priority: number) {
    updateMutation.mutate({ priority });
  }

  return (
    <div
      className="fixed inset-0 z-50 flex justify-end bg-black/50"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-[640px] h-full bg-panel border-l border-edge shadow-2xl flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-3 border-b border-edge shrink-0">
          <div className="flex items-center gap-2">
            <span className="text-[10px] uppercase tracking-wide text-muted">Card</span>
            {item && (
              <span className="text-[10px] text-muted font-mono">{item.id.slice(-8)}</span>
            )}
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded text-muted hover:text-white hover:bg-edge transition-colors"
            aria-label="Close"
          >
            <X size={16} />
          </button>
        </div>

        {isLoading || !item ? (
          <div className="flex-1 flex items-center justify-center text-sm text-muted">
            Loading…
          </div>
        ) : (
          <div className="flex-1 min-h-0 overflow-y-auto">
            <div className="px-5 py-4 space-y-4">
              {/* Title */}
              <Input
                value={title}
                onChange={(e) => {
                  setTitle(e.target.value);
                  setDirty(true);
                }}
                className="bg-transparent border-transparent border-b hover:border-edge focus:border-accent rounded-none px-0 py-0 pb-1 text-base font-semibold"
                placeholder="Card title"
              />

              {/* Status + actions */}
              <div className="flex flex-wrap items-center gap-2">
                <SimpleSelect
                  value={
                    item.columnId ??
                    columns.find((column) => column.role === item.status)?.id ??
                    ''
                  }
                  onValueChange={handleColumnChange}
                  className="w-auto rounded-md px-2.5 py-1 text-xs"
                  options={columns.map((column) => ({ value: column.id, label: column.name }))}
                />

                <SimpleSelect
                  value={item.kind}
                  onValueChange={(v) => handleKindChange(v as WorkItemKind)}
                  className="w-auto rounded-md px-2.5 py-1 text-xs"
                  options={KIND_OPTIONS.map((k) => ({ value: k.id, label: k.label }))}
                />

                <SimpleSelect
                  value={String(item.priority)}
                  onValueChange={(v) => handlePriorityChange(Number(v))}
                  className="w-auto rounded-md px-2.5 py-1 text-xs"
                  options={[
                    { value: '0', label: 'Low priority' },
                    { value: '1', label: 'Medium priority' },
                    { value: '2', label: 'High priority' },
                    { value: '3', label: 'Urgent' },
                  ]}
                />

                {item.status !== 'done' && (
                  <button
                    onClick={() => completeMutation.mutate()}
                    className="flex items-center gap-1 px-2.5 py-1 rounded-md bg-emerald-500/10 hover:bg-emerald-500/20 text-emerald-300 text-xs font-medium transition-colors"
                  >
                    <CheckCircle size={12} />
                    Complete
                  </button>
                )}
              </div>

              {/* Assignee */}
              <div className="flex items-center gap-2">
                <Bot size={14} className="text-muted" />
                <SimpleSelect
                  value={item.assigneeAgentId ?? ''}
                  onValueChange={handleAssigneeChange}
                  className="flex-1 rounded-md px-2.5 py-1 text-xs"
                  options={[
                    { value: '', label: 'Unassigned' },
                    ...agents.map((a) => ({ value: a.id, label: a.name })),
                  ]}
                />
              </div>

              {/* Description */}
              <div>
                <label className="block text-[10px] uppercase tracking-wide text-muted mb-1">
                  Description
                </label>
                <Textarea
                  value={description}
                  onChange={(e) => {
                    setDescription(e.target.value);
                    setDirty(true);
                  }}
                  rows={6}
                  placeholder="Add a description… (markdown supported)"
                  className="px-3 py-2 font-mono placeholder-muted"
                />
              </div>

              {/* Labels */}
              <div>
                <label className="block text-[10px] uppercase tracking-wide text-muted mb-1">
                  Labels (comma-separated)
                </label>
                <Input
                  value={labelsInput}
                  onChange={(e) => {
                    setLabelsInput(e.target.value);
                    setDirty(true);
                  }}
                  placeholder="e.g. backend, urgent, feature-x"
                  className="px-3 py-2 placeholder-muted"
                />
              </div>

              {/* Blocked reason */}
              {item.status === 'blocked' && item.blockedReason && (
                <div className="rounded-lg border border-red-400/30 bg-red-500/5 px-3 py-2">
                  <p className="text-[10px] uppercase tracking-wide text-red-300 font-semibold">
                    Blocked
                  </p>
                  <p className="mt-1 text-sm text-red-200">{item.blockedReason}</p>
                </div>
              )}

              {/* Save button */}
              {dirty && (
                <div className="flex items-center gap-2">
                  <button
                    onClick={handleSave}
                    disabled={updateMutation.isPending}
                    className="px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-50 text-white text-xs font-medium transition-colors"
                  >
                    {updateMutation.isPending ? 'Saving…' : 'Save changes'}
                  </button>
                  <button
                    onClick={() => {
                      if (!item) return;
                      setTitle(item.title);
                      setDescription(item.description ?? '');
                      setLabelsInput(item.labels.join(', '));
                      setDirty(false);
                    }}
                    className="text-xs text-muted hover:text-white transition-colors"
                  >
                    Discard
                  </button>
                </div>
              )}

              {/* Timestamps */}
              <div className="text-[10px] text-muted space-y-0.5 pt-2 border-t border-edge">
                <div>Created {new Date(item.createdAt).toLocaleString()}</div>
                {item.startedAt && (
                  <div>Started {new Date(item.startedAt).toLocaleString()}</div>
                )}
                {item.completedAt && (
                  <div>Completed {new Date(item.completedAt).toLocaleString()}</div>
                )}
              </div>

              {/* Comments */}
              <div className="pt-3 border-t border-edge">
                <h4 className="text-xs font-semibold text-white mb-2">Comments</h4>
                <WorkItemComments workItemId={workItemId} agents={agents} />
              </div>

              {/* Delete */}
              <div className="pt-3 border-t border-edge">
                <button
                  onClick={async () => {
                    if (!(await confirm(`Delete "${item.title}"?`))) return;
                    deleteMutation.mutate();
                  }}
                  className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-xs text-red-400 hover:bg-red-400/10 transition-colors"
                >
                  <Trash2 size={12} />
                  Delete card
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
