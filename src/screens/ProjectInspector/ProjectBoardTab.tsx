import { useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  DndContext,
  DragOverlay,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  pointerWithin,
  rectIntersection,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragStartEvent,
} from '@dnd-kit/core';
import { KanbanSquare, Plus } from 'lucide-react';
import { workItemsApi } from '../../api/workItems';
import { agentsApi } from '../../api/agents';
import { Agent, WorkItem, WorkItemStatus } from '../../types';
import { cn } from '../../lib/cn';
import { ProjectBoardCard } from './ProjectBoardCard';
import { ProjectBoardDetailDrawer } from './ProjectBoardDetailDrawer';

const COLUMNS: { id: WorkItemStatus; label: string; tone: string }[] = [
  { id: 'backlog', label: 'Backlog', tone: 'text-muted' },
  { id: 'todo', label: 'Todo', tone: 'text-secondary' },
  { id: 'in_progress', label: 'In Progress', tone: 'text-blue-300' },
  { id: 'blocked', label: 'Blocked', tone: 'text-red-300' },
  { id: 'review', label: 'Review', tone: 'text-amber-300' },
  { id: 'done', label: 'Done', tone: 'text-emerald-300' },
];

const CARD_DRAG_PREFIX = 'work-item:';
const COLUMN_DROP_PREFIX = 'column:';

function cardDragId(id: string): string {
  return `${CARD_DRAG_PREFIX}${id}`;
}
function parseCardDragId(id: string | number | null | undefined): string | null {
  if (typeof id !== 'string' || !id.startsWith(CARD_DRAG_PREFIX)) return null;
  return id.slice(CARD_DRAG_PREFIX.length);
}
function columnDropId(status: WorkItemStatus): string {
  return `${COLUMN_DROP_PREFIX}${status}`;
}
function parseColumnDropId(
  id: string | number | null | undefined,
): WorkItemStatus | null {
  if (typeof id !== 'string' || !id.startsWith(COLUMN_DROP_PREFIX)) return null;
  return id.slice(COLUMN_DROP_PREFIX.length) as WorkItemStatus;
}

export function ProjectBoardTab({ projectId }: { projectId: string }) {
  const queryClient = useQueryClient();
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [openItemId, setOpenItemId] = useState<string | null>(null);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor),
  );

  const { data: items = [], isLoading } = useQuery<WorkItem[]>({
    queryKey: ['work-items', projectId],
    queryFn: () => workItemsApi.list(projectId),
  });

  const { data: agents = [] } = useQuery<Agent[]>({
    queryKey: ['agents'],
    queryFn: agentsApi.list,
  });

  const agentById = useMemo(() => new Map(agents.map((a) => [a.id, a])), [agents]);

  const itemsByStatus = useMemo(() => {
    const groups = new Map<WorkItemStatus, WorkItem[]>();
    for (const col of COLUMNS) groups.set(col.id, []);
    for (const item of items) {
      if (item.status === 'cancelled') continue;
      const list = groups.get(item.status as WorkItemStatus);
      if (list) list.push(item);
    }
    for (const [, list] of groups) list.sort((a, b) => a.position - b.position);
    return groups;
  }, [items]);

  const moveMutation = useMutation({
    mutationFn: ({ id, status, position }: { id: string; status: WorkItemStatus; position?: number }) =>
      workItemsApi.move(id, status, position),
    onMutate: async ({ id, status, position }) => {
      await queryClient.cancelQueries({ queryKey: ['work-items', projectId] });
      const prev = queryClient.getQueryData<WorkItem[]>(['work-items', projectId]);
      queryClient.setQueryData<WorkItem[]>(['work-items', projectId], (old = []) =>
        old.map((it) =>
          it.id === id
            ? { ...it, status, position: position ?? it.position }
            : it,
        ),
      );
      return { prev };
    },
    onError: (_err, _vars, ctx) => {
      if (ctx?.prev) queryClient.setQueryData(['work-items', projectId], ctx.prev);
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['work-items', projectId] });
    },
  });

  const blockMutation = useMutation({
    mutationFn: ({ id, reason }: { id: string; reason: string }) =>
      workItemsApi.block(id, reason),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['work-items', projectId] });
    },
  });

  function handleDragStart(event: DragStartEvent) {
    setDraggingId(parseCardDragId(event.active.id));
  }

  function handleDragEnd(event: DragEndEvent) {
    setDraggingId(null);
    const cardId = parseCardDragId(event.active.id);
    if (!cardId || !event.over) return;

    const overId = event.over.id;
    let targetStatus = parseColumnDropId(overId);
    let targetCardId: string | null = null;
    if (!targetStatus) {
      const overCardId = parseCardDragId(overId);
      if (overCardId) {
        const overItem = items.find((it) => it.id === overCardId);
        if (overItem) {
          targetStatus = overItem.status as WorkItemStatus;
          targetCardId = overCardId;
        }
      }
    }
    if (!targetStatus) return;

    const card = items.find((it) => it.id === cardId);
    if (!card) return;

    if (targetStatus === 'blocked') {
      const reason = window.prompt('Why is this card blocked?');
      if (!reason || !reason.trim()) return;
      blockMutation.mutate({ id: cardId, reason: reason.trim() });
      return;
    }

    // Compute target position
    const columnItems = (itemsByStatus.get(targetStatus) ?? []).filter(
      (it) => it.id !== cardId,
    );
    let position: number | undefined;
    if (targetCardId) {
      const idx = columnItems.findIndex((it) => it.id === targetCardId);
      if (idx >= 0) {
        const before = idx > 0 ? columnItems[idx - 1].position : 0;
        const after = columnItems[idx].position;
        position = (before + after) / 2;
      }
    }
    // Same column same position no-op
    if (
      card.status === targetStatus &&
      (position === undefined || Math.abs(position - card.position) < 0.0001)
    ) {
      return;
    }
    moveMutation.mutate({ id: cardId, status: targetStatus, position });
  }

  const draggingItem = draggingId ? items.find((it) => it.id === draggingId) ?? null : null;

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-32 text-muted text-sm">Loading…</div>
    );
  }

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={(args) => {
        const pointerCollisions = pointerWithin(args);
        if (pointerCollisions.length > 0) return pointerCollisions;
        const intersect = rectIntersection(args);
        if (intersect.length > 0) return intersect;
        return closestCenter(args);
      }}
      onDragStart={handleDragStart}
      onDragEnd={handleDragEnd}
    >
      <div className="flex flex-col h-full">
        <div className="flex items-center justify-between px-4 py-3 border-b border-edge">
          <h3 className="flex items-center gap-2 text-sm font-semibold text-white">
            <KanbanSquare size={14} className="text-emerald-400" />
            Board
            <span className="text-xs text-muted font-normal">
              ({items.filter((it) => it.status !== 'cancelled').length})
            </span>
          </h3>
        </div>

        <div className="flex-1 min-h-0 overflow-x-auto overflow-y-hidden">
          <div className="flex gap-3 p-3 h-full min-w-max">
            {COLUMNS.map((col) => {
              const colItems = itemsByStatus.get(col.id) ?? [];
              return (
                <Column
                  key={col.id}
                  id={col.id}
                  label={col.label}
                  tone={col.tone}
                  count={colItems.length}
                >
                  {col.id === 'backlog' && (
                    <QuickAddRow projectId={projectId} />
                  )}
                  {colItems.map((item) => (
                    <DraggableCard key={item.id} id={item.id}>
                      <ProjectBoardCard
                        item={item}
                        assignee={item.assigneeAgentId ? agentById.get(item.assigneeAgentId) ?? null : null}
                        onClick={() => setOpenItemId(item.id)}
                      />
                    </DraggableCard>
                  ))}
                  {colItems.length === 0 && col.id !== 'backlog' && (
                    <div className="rounded-lg border border-dashed border-edge px-3 py-4 text-center text-[11px] text-muted">
                      Drop cards here
                    </div>
                  )}
                </Column>
              );
            })}
          </div>
        </div>
      </div>

      <DragOverlay>
        {draggingItem ? (
          <div className="rotate-1 opacity-90">
            <ProjectBoardCard
              item={draggingItem}
              assignee={draggingItem.assigneeAgentId ? agentById.get(draggingItem.assigneeAgentId) ?? null : null}
              onClick={() => {}}
            />
          </div>
        ) : null}
      </DragOverlay>

      {openItemId && (
        <ProjectBoardDetailDrawer
          projectId={projectId}
          workItemId={openItemId}
          agents={agents}
          onClose={() => setOpenItemId(null)}
        />
      )}
    </DndContext>
  );
}

// ── Column droppable ──────────────────────────────────────────────────────────

function Column({
  id,
  label,
  tone,
  count,
  children,
}: {
  id: WorkItemStatus;
  label: string;
  tone: string;
  count: number;
  children: React.ReactNode;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: columnDropId(id) });
  return (
    <div className="flex flex-col w-72 shrink-0 rounded-xl border border-edge bg-panel">
      <div className="flex items-center justify-between px-3 py-2 border-b border-edge">
        <div className="flex items-center gap-2">
          <span className={cn('text-xs font-semibold uppercase tracking-wide', tone)}>
            {label}
          </span>
          <span className="text-[10px] text-muted tabular-nums">{count}</span>
        </div>
      </div>
      <div
        ref={setNodeRef}
        className={cn(
          'flex-1 min-h-0 overflow-y-auto p-2 space-y-2 transition-colors',
          isOver && 'bg-accent/5',
        )}
      >
        {children}
      </div>
    </div>
  );
}

// ── Draggable card wrapper ────────────────────────────────────────────────────

function DraggableCard({
  id,
  children,
}: {
  id: string;
  children: React.ReactNode;
}) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id: cardDragId(id),
  });
  return (
    <div
      ref={setNodeRef}
      {...attributes}
      {...listeners}
      className={cn('touch-none', isDragging && 'opacity-30')}
    >
      {children}
    </div>
  );
}

// ── Quick-add row at top of Backlog ───────────────────────────────────────────

function QuickAddRow({ projectId }: { projectId: string }) {
  const queryClient = useQueryClient();
  const [title, setTitle] = useState('');
  const [adding, setAdding] = useState(false);

  const createMutation = useMutation({
    mutationFn: (titleValue: string) =>
      workItemsApi.create({ projectId, title: titleValue, status: 'backlog' }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['work-items', projectId] });
      setTitle('');
      setAdding(false);
    },
  });

  if (!adding) {
    return (
      <button
        onClick={() => setAdding(true)}
        className="w-full flex items-center gap-2 px-2.5 py-1.5 rounded-md text-xs text-muted hover:text-accent-hover hover:bg-accent/10 transition-colors"
      >
        <Plus size={12} />
        Add card
      </button>
    );
  }

  return (
    <div className="rounded-md border border-accent/40 bg-background px-2 py-1.5">
      <input
        autoFocus
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && title.trim()) {
            createMutation.mutate(title.trim());
          } else if (e.key === 'Escape') {
            setTitle('');
            setAdding(false);
          }
        }}
        onBlur={() => {
          if (!title.trim()) setAdding(false);
        }}
        placeholder="Card title…"
        className="w-full bg-transparent text-xs text-white outline-none placeholder-muted"
      />
    </div>
  );
}
