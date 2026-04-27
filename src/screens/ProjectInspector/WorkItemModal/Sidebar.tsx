import { useMemo } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { AlertOctagon, Bot, Trash2 } from 'lucide-react';
import { confirm } from '../../../lib/dialog';
import { workItemsApi } from '../../../api/workItems';
import type { Agent, ProjectBoardColumn, WorkItem, WorkItemKind } from '../../../types';
import { SimpleSelect } from '../../../components/ui';
import { LabelChipPicker } from './LabelChipPicker';
import { PRIORITY_OPTIONS } from './PriorityBadge';

const KIND_OPTIONS: { id: WorkItemKind; label: string }[] = [
  { id: 'task', label: 'Task' },
  { id: 'bug', label: 'Bug' },
  { id: 'story', label: 'Story' },
  { id: 'spike', label: 'Spike' },
  { id: 'chore', label: 'Chore' },
];

interface Props {
  item: WorkItem;
  projectId: string;
  columns: ProjectBoardColumn[];
  agents: Agent[];
  labels: string[];
  labelSuggestions: string[];
  onLabelsChange: (next: string[]) => void;
  onKindChange: (kind: WorkItemKind) => void;
  onPriorityChange: (priority: number) => void;
  onColumnChange: (columnId: string) => void;
  onDeleted: () => void;
}

function Row({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-1">
      <div className="text-[10px] uppercase tracking-wide text-muted">{label}</div>
      {children}
    </div>
  );
}

export function Sidebar({
  item,
  columns,
  agents,
  labels,
  labelSuggestions,
  onLabelsChange,
  onKindChange,
  onPriorityChange,
  onColumnChange,
  onDeleted,
}: Props) {
  const queryClient = useQueryClient();

  const currentColumnId = useMemo(() => {
    if (item.columnId) return item.columnId;
    return columns.find((c) => c.role === item.status)?.id ?? '';
  }, [item.columnId, item.status, columns]);

  const claimMutation = useMutation({
    mutationFn: ({ agentId }: { agentId: string }) => workItemsApi.claim(item.id, agentId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['work-items'] });
      queryClient.invalidateQueries({ queryKey: ['work-items', item.id, 'events'] });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: () => workItemsApi.delete(item.id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['work-items'] });
      onDeleted();
    },
  });

  return (
    <aside className="flex w-[260px] shrink-0 flex-col gap-4 overflow-y-auto border-l border-edge bg-background/30 px-4 py-4 text-sm">
      {item.status === 'blocked' && item.blockedReason && (
        <div className="rounded-lg border border-red-400/30 bg-red-500/5 px-3 py-2">
          <div className="flex items-center gap-1 text-[10px] font-semibold uppercase tracking-wide text-red-300">
            <AlertOctagon size={11} /> Blocked
          </div>
          <p className="mt-1 text-xs text-red-200">{item.blockedReason}</p>
        </div>
      )}

      <Row label="Assignee">
        <div className="flex items-center gap-2">
          <Bot size={13} className="text-muted" />
          <SimpleSelect
            value={item.assigneeAgentId ?? ''}
            onValueChange={(v) => v && claimMutation.mutate({ agentId: v })}
            className="flex-1 rounded-md px-2 py-1 text-xs"
            options={[
              { value: '', label: 'Unassigned' },
              ...agents.map((a) => ({ value: a.id, label: a.name })),
            ]}
          />
        </div>
      </Row>

      <Row label="Status">
        <SimpleSelect
          value={currentColumnId}
          onValueChange={(v) => v && onColumnChange(v)}
          className="w-full rounded-md px-2 py-1 text-xs"
          options={columns.map((c) => ({ value: c.id, label: c.name }))}
        />
      </Row>

      <Row label="Kind">
        <SimpleSelect
          value={item.kind}
          onValueChange={(v) => onKindChange(v as WorkItemKind)}
          className="w-full rounded-md px-2 py-1 text-xs"
          options={KIND_OPTIONS.map((k) => ({ value: k.id, label: k.label }))}
        />
      </Row>

      <Row label="Priority">
        <SimpleSelect
          value={String(item.priority)}
          onValueChange={(v) => onPriorityChange(Number(v))}
          className="w-full rounded-md px-2 py-1 text-xs"
          options={PRIORITY_OPTIONS}
        />
      </Row>

      <Row label="Labels">
        <LabelChipPicker
          value={labels}
          onChange={onLabelsChange}
          suggestions={labelSuggestions}
        />
      </Row>

      <div className="border-t border-edge pt-3 text-[10px] text-muted space-y-0.5">
        <div>Created {new Date(item.createdAt).toLocaleString()}</div>
        {item.startedAt && <div>Started {new Date(item.startedAt).toLocaleString()}</div>}
        {item.completedAt && <div>Completed {new Date(item.completedAt).toLocaleString()}</div>}
      </div>

      <div className="mt-auto border-t border-edge pt-3">
        <button
          type="button"
          onClick={async () => {
            if (!(await confirm(`Delete "${item.title}"?`, { kind: 'warning' }))) return;
            deleteMutation.mutate();
          }}
          className="flex w-full items-center gap-1.5 rounded-md px-2 py-1.5 text-xs text-red-400 transition-colors hover:bg-red-400/10"
        >
          <Trash2 size={12} />
          Delete card
        </button>
      </div>
    </aside>
  );
}
